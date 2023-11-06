// This is code for a rust-only implementation of stage ASSEMBLE_VDJ.
//
// ◼ The way we generate a bam file here doesn't make sense.  In the current
// ◼ implementation, we build sorted bam files, then merge them, which involves
// ◼ interweaving the files.  It seems like we could avoid the interweaving.  For
// ◼ each chunk, we could create two sorted bam files, one for the placed reads and
// ◼ one for the unplaced reads.  Then in the join we should be able to
// ◼ 'samtools reheader' the bam files, then 'samtools cat' all the placed bam
// ◼ files, followed by the unplaced bam files, and that should be sorted without
// ◼ doing any interweaving.

use crate::assembly_types::{
    AsmReadsPerBcFormat, AssemblyStageInputs, BamBaiFile, BamFile, FastqFile,
};
use crate::contig_aligner::ContigAligner;
use anyhow::Result;
use barcode::HasBarcode;
use cr_types::rna_read::RnaRead;
use cr_types::MetricsFile;
use debruijn::dna_string::DnaString;
use debruijn::kmer::Kmer20;
use debruijn::Mer;
use io_utils::{fwrite, fwriteln, path_exists, read_obj, read_to_string_safe, write_obj};
use itertools::Itertools;
use kmer_lookup::make_kmer_lookup_20_single;
use libc::{getrlimit, rlimit, RLIMIT_NOFILE};
use martian::{MartianFileType, MartianRover, MartianStage, Resource, StageDef};
use martian_derive::{make_mro, martian_filetype, MartianStruct};
use martian_filetypes::bin_file::{BinaryFormat, BincodeFile};
use martian_filetypes::json_file::JsonFile;
use martian_filetypes::lz4_file::Lz4;
use martian_filetypes::tabular_file::{CsvFile, TsvFile, TsvFileNoHeader};
use martian_filetypes::{FileTypeRead, FileTypeWrite, LazyFileTypeIO, LazyWrite};
use metric::{JsonReporter, Metric, SerdeFormat, SimpleHistogram};
use parameters_toml::vdj_max_reads_per_barcode;
use perf_stats::{available_mem_gb, elapsed, mem_usage_gb, peak_mem_usage_gb, ps_me};
use pretty_trace::{new_thread_message, CHashMap, PrettyTrace};
use rust_htslib::bam;
use rust_htslib::bam::HeaderView;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{remove_file, rename, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::ThreadId;
use std::time::Instant;
use std::{env, fs, thread};
use string_utils::{stringme, strme, TextUtils};
use vdj_ann::annotate::{chain_type, ContigAnnotation, JunctionSupport};
use vdj_ann::refx::{make_vdj_ref_data_core, RefData};
use vdj_asm_utils::asm::write_sam_record_simple;
use vdj_asm_utils::barcode_data::{metrics_json, BarcodeData, BarcodeDataBrief, BarcodeDataSum};
use vdj_asm_utils::constants::{
    ReadType, CHAIN_TYPESX, CLIP, GAP_EXTEND, GAP_OPEN, KMER_LEN_BANDED_ALIGN, MATCH_SCORE,
    MISMATCH_SCORE, OR_CHAIN_TYPES, WINDOW_SIZE_BANDED_ALIGN,
};
use vdj_asm_utils::heuristics::Heuristics;
use vdj_asm_utils::hops::FlowcellContam;
use vdj_asm_utils::log_opts::LogOpts;
use vdj_asm_utils::primers::{get_primer_exts, inner_primers, outer_primers};
use vdj_asm_utils::process::process_barcode;
use vdj_asm_utils::{bam_utils, graph_read, sw};
use vdj_reference::VdjReceptor;
use vdj_types::VdjChain;
pub struct Assembly;
martian_filetype!(Lz4File, "lz4");
martian_filetype!(TxtFile, "txt");
martian_filetype!(BinFile, "bin");

martian_filetype!(_BarcodeDataBriefFile, "bdf");
pub type BarcodeDataBriefFile = BinaryFormat<_BarcodeDataBriefFile, Vec<BarcodeDataBrief>>;

// martian_filetype!(_ContigAnnFile, "can");
// impl FileStorage<Vec<ContigAnnotation>> for _ContigAnnFile {}
// pub type ContigAnnFormat = BinaryFormat<_ContigAnnFile>;

// =================================================================================
// FUNCTION TO MERGE BAMS
// =================================================================================

// merge coordinate-sorted bam files to yield a new coordinate-sorted bam file
//
// ◼ Duplicated code, with a couple of changes to get it to compile.  If
// ◼ we're going to use it, it shouldn't be in two places.
//
// ◼ sort_bam and index_bam also copied
//
// Note that this uses FOUR threads.  Not tested to determine if this makes it
// faster.

fn merge_bams(paths: &[BamFile], out: &Path) -> Result<()> {
    let mut paths = paths
        .iter()
        .map(|p| PathBuf::from(p.as_ref()))
        .collect::<Vec<_>>();
    // if there's only a single input path, copy the input
    if paths.len() == 1 {
        let _ = std::fs::copy(&paths[0], out)?;
        return Ok(());
    }
    // get the NOFILE ulimit so we don't ask samtools to open too many files
    let rlim = unsafe {
        let mut rlim = rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let ret = getrlimit(RLIMIT_NOFILE, &mut rlim as *mut rlimit);
        if ret != 0 {
            panic!("unable to determine soft NOFILE ulimit");
        }
        rlim.rlim_cur as usize
    };
    if rlim < 102 {
        panic!("soft NOFILE ulimit is unworkably low");
    }
    let rlim = rlim - 100;
    // keep merging files, rlim at a time, until only 1 remains
    let mut created = vec![];
    while paths.len() > 1 {
        let mut fofn = PathBuf::from(out);
        fofn.set_extension("fofn");
        let fofn = fofn.to_str().unwrap();
        {
            let mut handle = File::create(fofn)?;
            let n = paths.len();
            let rest = paths.split_off(rlim.min(n));
            for path in paths {
                handle.write_all(path.to_str().unwrap().as_bytes())?;
                handle.write_all(b"\n")?;
            }
            paths = rest;
        }
        let mut bam = PathBuf::from(out);
        bam.set_extension(format!("{}.bam", created.len()));
        created.push(bam.clone());
        paths.push(bam.clone());
        Command::new("samtools")
            .args([
                "merge",
                "-@",
                "3",
                "-c",
                "-p",
                "-s",
                "0",
                "-b",
                fofn,
                bam.to_str().unwrap(),
            ])
            .output()
            .expect("failed to merge bam files!");
        remove_file(fofn)?;
    }
    // move the final file into position
    rename(&paths[0], out)?;
    // remove all other intermediate bam files
    for bam in created {
        if bam != paths[0] {
            remove_file(bam)?;
        }
    }
    Ok(())
}

// modified to use only one thread, and to reduce memory usage

fn sort_bam(input: &str, output: &str) {
    println!("running samtools sort -l 8G -o {output} {input}");
    Command::new("samtools")
        .args(["sort", "-l", "8G", "-m", "600M", "-o", output, input])
        .output()
        .unwrap_or_else(|_| panic!("failed to sort {}", &input));
}

// Note that index_bam uses FOUR threads.  Not tested to determine if this makes
// it faster.

fn index_bam(bam: &Path) {
    Command::new("samtools")
        .args(["index", "-@", "3", bam.to_str().unwrap()])
        .output()
        .unwrap_or_else(|_| panic!("failed to index {}", bam.display()));
}

pub fn line_by_line_copy<R, W>(reader: &mut R, writer: &mut BufWriter<W>) -> Result<()>
where
    R: BufRead,
    W: Write,
{
    for line in reader.lines() {
        writeln!(writer, "{}", line?)?;
    }
    Ok(())
}

// =================================================================================
// FIND ENRICHMENT PRIMERS
// =================================================================================

// ◼ This should be run only once, but it's not.

fn enrichment_primers(
    primer_file: Option<&PathBuf>,
    refdata: &RefData,
    is_tcr: bool,
    is_bcr: bool,
    inner_primersx: &mut Vec<Vec<u8>>,
    outer_primersx: &mut Vec<Vec<u8>>,
) {
    // Specify inner primers.  If the customer has not specified primers, we use the reference
    // sequence to decide if the species is human or mouse.

    let iprimers = match primer_file {
        Some(path) => BufReader::new(File::open(path).unwrap())
            .lines()
            .map(Result::unwrap)
            .collect(),
        None => Vec::<String>::new(),
    };
    if iprimers.is_empty() {
        let (mut is_human, mut is_mouse) = (false, false);
        let (mut human_count, mut mouse_count) = (0, 0);
        let mut human_inner_primers = inner_primers("human", "tcr");
        human_inner_primers.append(&mut inner_primers("human", "bcr"));
        let mut mouse_inner_primers = inner_primers("mouse", "tcr");
        mouse_inner_primers.append(&mut inner_primers("mouse", "bcr"));
        for i in 0..refdata.refs.len() {
            if !refdata.is_c(i) {
                continue;
            }
            let x = refdata.refs[i].clone().rc().to_string();
            for primer in &human_inner_primers {
                if x.contains(&stringme(primer)) {
                    human_count += 1;
                }
            }
            for primer in &mouse_inner_primers {
                if x.contains(&stringme(primer)) {
                    mouse_count += 1;
                }
            }
        }
        if human_count > 0 {
            is_human = true;
        }
        if mouse_count > 0 {
            is_mouse = true;
        }
        if is_human && is_tcr {
            inner_primersx.append(&mut inner_primers("human", "tcr"));
            outer_primersx.append(&mut outer_primers("human", "tcr"));
        }
        if is_human && is_bcr {
            inner_primersx.append(&mut inner_primers("human", "bcr"));
            outer_primersx.append(&mut outer_primers("human", "bcr"));
        }
        if is_mouse && is_tcr {
            inner_primersx.append(&mut inner_primers("mouse", "tcr"));
            outer_primersx.append(&mut outer_primers("mouse", "tcr"));
        }
        if is_mouse && is_bcr {
            inner_primersx.append(&mut inner_primers("mouse", "bcr"));
            outer_primersx.append(&mut outer_primers("mouse", "bcr"));
        }
    } else {
        for x in iprimers {
            let p = x.as_bytes().to_vec();
            inner_primersx.push(p);
        }
    }
}

fn sam_to_bam(out_bam_file: &Path, sam_header: bam::header::Header, out_sam_filenamex: &Path) {
    // Convert sam to bam.
    // ◼ The whole business of first writing sam.lz4, then converting
    // ◼ to bam here seems nuts and inefficient.

    let out_bam_filename_str = out_bam_file.to_str().unwrap();
    let mut out = bam::Writer::from_path(out_bam_file, &sam_header, bam::Format::Bam).unwrap();
    let h = HeaderView::from_header(&sam_header);
    let fin = File::open(out_sam_filenamex).expect("Failed to open file for reading");
    let mut fin = lz4::Decoder::new(fin).expect("Failed to create lz4 decoder");
    let fin = BufReader::new(&mut fin);
    for line in fin.lines() {
        let s = line.unwrap();
        let t = s.as_bytes();
        let rec: bam::Record = bam::Record::from_sam(&h, t).unwrap();
        out.write(&rec).unwrap();
    }
    drop(out);
    drop(sam_header);
    drop(h);

    // Sort the bam file.  This can use a lot of memory.  We delete a temporary file if it
    // exists in case you're debugging this code, as if it's left around it will crash
    // samtools.

    println!("sorting bam, mem = {:.2}", mem_usage_gb());
    let out_bam_sorted_filename =
        out_bam_filename_str.rev_before("/").to_string() + "/contig_bam_sorted.bam";
    let tmp_filename = out_bam_sorted_filename.clone() + ".tmp.0000.bam";
    if path_exists(&tmp_filename) {
        remove_file(&tmp_filename).unwrap();
    }
    sort_bam(out_bam_filename_str, &out_bam_sorted_filename);
    rename(&out_bam_sorted_filename, out_bam_file).unwrap();
}

fn get_max_reads_per_barcode(paired_end: bool) -> f64 {
    match paired_end {
        true => (*vdj_max_reads_per_barcode().unwrap() / 2) as f64,
        false => (*vdj_max_reads_per_barcode().unwrap()) as f64,
    }
}
// =================================================================================
// DEFINE THE STAGE INPUTS AND OUTPUTS
// =================================================================================

#[derive(Debug, Serialize, Deserialize, MartianStruct)]
pub struct AssemblyStageOutputs {
    pub contig_bam: BamFile,
    pub contig_bam_bai: BamBaiFile,
    pub summary_tsv: TsvFile<()>,
    pub umi_summary_tsv: TsvFile<()>,
    pub metrics_summary_json: MetricsFile,
    pub contig_annotations: JsonFile<Vec<ContigAnnotation>>,
    pub barcode_brief: BarcodeDataBriefFile,
    pub barcode_support: CsvFile<()>,
    pub barcodes_in_chunks: Vec<JsonFile<Vec<String>>>,
    pub assemblable_reads_per_bc: AsmReadsPerBcFormat,

    // The outputs below are simply bubbled up to the outs folder
    pub align_info: TxtFile,
    pub unmapped_sample_fastq: FastqFile,
    pub report: TxtFile,
}

// =================================================================================
// DEFINE THE CHUNK INPUTS AND OUTPUTS
// =================================================================================

#[derive(Clone, Serialize, Deserialize, MartianStruct)]
pub struct AssemblyChunkInputs {
    pub chunk_rna_reads: Lz4<BincodeFile<Vec<RnaRead>>>,
    pub perf_track: Option<bool>,
    pub chunk_id: usize,
}

#[derive(Clone, Serialize, Deserialize, MartianStruct)]
pub struct AssemblyChunkOutputs {
    pub contig_bam: BamFile,
    pub summary_tsv: TsvFileNoHeader<()>,
    pub umi_summary_tsv: TsvFileNoHeader<()>,
    pub contig_annotations: JsonFile<Vec<ContigAnnotation>>,
    pub barcodes_in_chunk: JsonFile<Vec<String>>,
    pub align_info: TxtFile,
    pub unmapped_sample_fastq: FastqFile,
    pub barcode_data: BinFile,
    pub barcode_data_sum: BinFile,
    pub barcode_data_brief: BinFile,
    pub outs_builder: BincodeFile<Vec<AssemblyOutsBuilder>>,
}

// =================================================================================
// Intermediate output
// =================================================================================
#[derive(Serialize, Deserialize)]
pub struct AssemblyOutsBuilder {
    barcode: String,
    umi_count: usize,
}

// =================================================================================
// STAGE CODE BOILERPLATE
// =================================================================================

#[make_mro(stage_name = ASSEMBLE_VDJ)]
impl MartianStage for Assembly {
    type StageInputs = AssemblyStageInputs;
    type StageOutputs = AssemblyStageOutputs;
    type ChunkInputs = AssemblyChunkInputs;
    type ChunkOutputs = AssemblyChunkOutputs;

    // =============================================================================
    // THIS IS THE SPLIT CODE
    // =============================================================================

    fn split(
        &self,
        args: Self::StageInputs,
        rover: MartianRover,
    ) -> Result<StageDef<Self::ChunkInputs>> {
        // Set up tracebacks.
        if thread::current().name().unwrap() == "main" {
            let long_log: String = rover.make_path("../_full_traceback");
            PrettyTrace::new().noexit().full_file(&long_log).on();
        } else {
            PrettyTrace::new().noexit().on();
        }

        // Set up chunks.
        // ◼ Join memory highwater mark was 5.4 GB (rounded).
        // ◼ See comments about memory inefficiency in the join step.
        // 4 threads in join for `samtools merge/index`
        Ok(args
            .bc_sorted_rna_reads
            .into_iter()
            .enumerate()
            .map(|(i, chunk_rna_reads)| {
                (
                    AssemblyChunkInputs {
                        chunk_rna_reads,
                        perf_track: Some(false),
                        chunk_id: i,
                    },
                    Resource::with_mem_gb(2),
                )
            })
            .collect::<StageDef<_>>()
            .join_resource(Resource::with_mem_gb(6).threads(4)))
    }

    // =============================================================================
    // THIS IS THE CHUNK CODE
    // =============================================================================

    fn main(
        &self,
        args: Self::StageInputs,
        split_args: Self::ChunkInputs,
        rover: MartianRover,
    ) -> Result<Self::ChunkOutputs> {
        // Print some environment info.

        println!("\nstarting vdj_asm_asm, mem = {:.2}", mem_usage_gb());
        match available_mem_gb() {
            None => {
                println!("available mem = unknown");
            }
            Some(m) => {
                println!("available mem = {m:.2} GB");
            }
        }
        if split_args.perf_track == Some(true) {
            println!("host = {}", hostname::get().unwrap().to_string_lossy());
        }

        // Figure out chunk directory path.

        let chunk_dir = rover.files_path().to_str().unwrap().rev_before("/");

        // Force panic to yield a traceback, and make it a pretty one.

        let thread_message = new_thread_message();
        if thread::current().name().unwrap() == "main" {
            PrettyTrace::new()
                .noexit()
                .message(thread_message)
                .full_file(&format!("{chunk_dir}/_full_traceback"))
                .on();
        } else {
            PrettyTrace::new().noexit().on();
        }

        // Print the command.
        println!("{}", env::args().collect::<Vec<_>>().join(" "));

        // Load results from assembly prep stage.

        println!("n50_n50_rpu = {}", args.n50_n50_rpu);
        let is_tcr =
            args.receptor == Some(VdjReceptor::TR) || args.receptor == Some(VdjReceptor::TRGD);
        let is_bcr = args.receptor == Some(VdjReceptor::IG);
        let is_gd = Some(args.receptor == Some(VdjReceptor::TRGD));
        let (refdata, refdatax, refdata_full, rkmers_plus_full_20) =
            load_refdata(args.vdj_reference_path.as_ref(), is_tcr, is_bcr);
        let refs = &refdata.refs;

        // Specify inner primers.  If the customer has not specified primers, we use the
        // reference sequence to decide if the species is human or mouse.

        let mut inner_primersx = Vec::<Vec<u8>>::new();
        let mut outer_primersx = Vec::<Vec<u8>>::new();
        enrichment_primers(
            args.inner_enrichment_primers.as_ref(),
            &refdata,
            is_tcr,
            is_bcr,
            &mut inner_primersx,
            &mut outer_primersx,
        );

        // Get filenames and set up writers.

        let contig_annotations_file: JsonFile<Vec<ContigAnnotation>> =
            rover.make_path("contig_annotations");

        let umi_summary_file: TsvFileNoHeader<_> = rover.make_path("umi_summary");

        let summary_file: TsvFileNoHeader<_> = rover.make_path("summary");

        let ob_file: BincodeFile<_> = rover.make_path("outs_builder");

        // Set up to write a sam file.

        let out_sam_filenamex: Lz4File = rover.make_path("contig_sam.lz4");

        // Start of new code to process all barcodes.

        let t = Instant::now();
        let mut log_opts = LogOpts::new();
        if split_args.perf_track == Some(true) {
            log_opts.clock = true;
            log_opts.mem = true;
            log_opts.keep_all = true;
        }

        // Set up for alignment.

        let align_info_file: TxtFile = rover.make_path("align_info");

        let unmapped_sample_fastq_file: FastqFile = rover.make_path("unmapped_sample_fastq");

        let (sam_header, barcodes, barcode_data, barcode_data_brief) = write_simple_sam(
            &args,
            &split_args,
            &out_sam_filenamex,
            &contig_annotations_file,
            &align_info_file,
            &umi_summary_file,
            &summary_file,
            &ob_file,
            &unmapped_sample_fastq_file,
            &refdata,
            refdatax,
            refdata_full,
            inner_primersx,
            outer_primersx,
            rkmers_plus_full_20,
            is_tcr,
            is_bcr,
            is_gd,
            !refs.is_empty(),
            &log_opts,
            thread_message,
        )?;

        // Write barcode data.

        let barcode_data_file = rover.make_path("barcode_data.bin");
        write_obj(&barcode_data, &barcode_data_file);
        let barcode_data_sum_file = rover.make_path("barcode_data_sum.bin");
        let barcode_data_sum = BarcodeDataSum::sum(&barcode_data, &refdata);
        drop(barcode_data);
        write_obj(&barcode_data_sum, &barcode_data_sum_file);
        let barcode_data_brief_file = rover.make_path("barcode_data_brief.bin");
        write_obj(&barcode_data_brief, &barcode_data_brief_file);
        drop(barcode_data_brief);

        let out_bam_filename: BamFile = rover.make_path("contig_bam");
        sam_to_bam(&out_bam_filename, sam_header, &out_sam_filenamex);
        remove_file(&out_sam_filenamex).unwrap();

        // Create barcodes_in_chunk.json.

        let barcodes_in_chunk_file: JsonFile<_> = rover.make_path("barcodes_in_chunk");
        barcodes_in_chunk_file.write(&barcodes)?;
        drop(barcodes);

        // Print.

        println!(
            "\n▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓\
             ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓\n"
        );
        println!(
            "{:.3} seconds used processing barcodes, peak mem = {:.2}",
            elapsed(&t),
            peak_mem_usage_gb()
        );

        // See how much memory is in use, for debugging.

        if log_opts.clock {
            ps_me();
            println!(
                "\nexiting main program, mem = {:.2}, peak = {:.2}",
                mem_usage_gb(),
                peak_mem_usage_gb()
            );
        }
        Ok(AssemblyChunkOutputs {
            contig_bam: out_bam_filename,
            summary_tsv: summary_file,
            umi_summary_tsv: umi_summary_file,
            contig_annotations: contig_annotations_file,
            barcodes_in_chunk: barcodes_in_chunk_file,
            align_info: align_info_file,
            unmapped_sample_fastq: unmapped_sample_fastq_file,
            barcode_data: barcode_data_file,
            barcode_data_sum: barcode_data_sum_file,
            barcode_data_brief: barcode_data_brief_file,
            outs_builder: ob_file,
        })
    }

    // =============================================================================
    // THIS IS THE JOIN CODE
    // =============================================================================

    fn join(
        &self,
        args: Self::StageInputs,
        _chunk_defs: Vec<Self::ChunkInputs>,
        chunk_outs: Vec<Self::ChunkOutputs>,
        rover: MartianRover,
    ) -> Result<Self::StageOutputs> {
        // Set up logging.

        if thread::current().name().unwrap() == "main" {
            let long_log: String = rover.make_path("../_full_traceback");
            PrettyTrace::new().noexit().full_file(&long_log).on();
        } else {
            PrettyTrace::new().noexit().on();
        }
        let t = Instant::now();
        let log_opts = LogOpts::new();
        log_opts.report_perf_stats_now(&t, "upon entering join");

        // Determine if single end.  Flaky.

        let single_end = !args.paired_end;

        // Get the number of read pairs.  This is from the beginning, before we
        // throw out non-whitelisted barcodes.
        let total_read_pairs = args.total_read_pairs as usize;

        // Merge summary_tsv and umi_summary_tsv files.

        let summary_tsv_file: TsvFile<_> = rover.make_path("summary_tsv");
        write_summary_tsv(&summary_tsv_file, &chunk_outs)?;

        let umi_summary_tsv_file: TsvFile<_> = rover.make_path("umi_summary_tsv");
        write_umi_summary_tsv(&umi_summary_tsv_file, &chunk_outs)?;

        // Merge align info files.

        let align_info_file: TxtFile = rover.make_path("align_info");
        let mut align_info = align_info_file.buf_writer()?;
        for co in &chunk_outs {
            line_by_line_copy(&mut co.align_info.buf_reader()?, &mut align_info)?;
        }
        drop(align_info);

        // Set up report file.

        let report_file: TxtFile = rover.make_path("report");
        let mut report = report_file.buf_writer()?;

        // Merge unmapped fastq files.

        let unmapped_sample_fastq_file: FastqFile = rover.make_path("unmapped_sample_fastq");
        let mut unmapped_sample_fastq = unmapped_sample_fastq_file.buf_writer()?;
        for co in &chunk_outs {
            line_by_line_copy(
                &mut co.unmapped_sample_fastq.buf_reader()?,
                &mut unmapped_sample_fastq,
            )?;
        }
        drop(unmapped_sample_fastq);

        let mut barcode_data_brief = Vec::<BarcodeDataBrief>::new();
        for co in &chunk_outs {
            let mut barcode_data_brief_this: Vec<BarcodeDataBrief> =
                read_obj(&co.barcode_data_brief);
            barcode_data_brief.append(&mut barcode_data_brief_this);
        }

        // Load the number of read pairs assigned to each barcode.

        let bc_counts = args.corrected_bc_counts.as_ref().to_str().unwrap();
        let reader = BufReader::new(File::open(bc_counts).unwrap());
        let npairs: serde_json::Value = serde_json::from_reader(reader)?;

        // Merge contig annotation json files.  This also makes some adjustments:
        // 1. Add read count for barcode.
        // 2. Filter unrelated chains based on Gamma/Delta mode
        //      -> in GD mode, filter all AB (NOT VICE VERSA)
        //      -> in AB mode, no filter is applied to maintain consistency
        let gd_mode = args.receptor == Some(VdjReceptor::TRGD);

        let contig_annotations_file: JsonFile<_> = rover.make_path("contig_annotations");
        let mut ann_writer = contig_annotations_file.lazy_writer()?;
        for co in &chunk_outs {
            // In GD mode, take a pass to identify cells which have at least one productive
            // G/D chain

            let mut gd_barcodes = HashSet::new();
            if gd_mode {
                for can in co.contig_annotations.lazy_reader()? {
                    let can: ContigAnnotation = can?;
                    if can.productive.unwrap_or(false)
                        && can
                            .annotations
                            .iter()
                            .any(|ann| matches!(ann.feature.chain, VdjChain::TRG | VdjChain::TRD))
                    {
                        gd_barcodes.insert(can.barcode);
                    }
                }
            }

            // second pass
            let reader2 = co.contig_annotations.lazy_reader()?;
            let max_read_pairs_per_barcode = get_max_reads_per_barcode(args.paired_end);
            for can in reader2 {
                let mut can: ContigAnnotation = can?;
                let used = npairs[&can.barcode].as_u64().unwrap() as usize;
                let frac = if used as f64 <= max_read_pairs_per_barcode {
                    1.0_f64
                } else {
                    max_read_pairs_per_barcode / (used as f64)
                };
                can.fraction_of_reads_for_this_barcode_provided_as_input_to_assembly = Some(frac);
                ann_writer.write_item(&can)?;
            }
        }
        ann_writer.finish()?;

        // Merge bam files, then index.

        let out_bam_filename: BamFile = rover.make_path("contig_bam.bam");
        log_opts.report_perf_stats_now(&t, "before merging bam files");
        let bams: Vec<_> = chunk_outs.iter().map(|co| co.contig_bam.clone()).collect();
        let contig_bam_bai_filename: BamBaiFile = rover.make_path("contig_bam.bam.bai");
        if !chunk_outs.is_empty() {
            merge_bams(&bams, out_bam_filename.as_ref()).unwrap();
            log_opts.report_perf_stats_now(&t, "before indexing bam");
            index_bam(&out_bam_filename);
            log_opts.report_perf_stats_now(&t, "after sorting");
        } else {
            out_bam_filename.buf_writer()?;
        }

        let mut refdata = RefData::new();

        let is_tcr =
            args.receptor == Some(VdjReceptor::TR) || args.receptor == Some(VdjReceptor::TRGD);
        let is_bcr = args.receptor == Some(VdjReceptor::IG);
        let is_gd = Some(args.receptor == Some(VdjReceptor::TRGD));

        if let Some(ref ref_path) = args.vdj_reference_path {
            let fasta = read_to_string_safe(format!("{}/fasta/regions.fa", ref_path.display()));
            make_vdj_ref_data_core(&mut refdata, &fasta, "", is_tcr, is_bcr, None);
        }

        // Read in barcode data sum files and merge.  Then generate metrics from
        // that.

        log_opts.report_perf_stats_now(&t, "before reading barcode data");
        let mut barcode_data_sum = Vec::<BarcodeDataSum>::new();
        for co in &chunk_outs {
            let barcode_data_sum_this: BarcodeDataSum = read_obj(&co.barcode_data_sum);
            barcode_data_sum.push(barcode_data_sum_this);
        }

        log_opts.report_perf_stats_now(&t, "after reading");
        let mut inner_primersx = Vec::<Vec<u8>>::new();
        let mut outer_primersx = Vec::<Vec<u8>>::new();
        enrichment_primers(
            args.inner_enrichment_primers.as_ref(),
            &refdata,
            is_tcr,
            is_bcr,
            &mut inner_primersx,
            &mut outer_primersx,
        );
        let mut json = Vec::<u8>::new();

        println!("Just before metrics json");
        metrics_json(
            rover.files_path().to_str().unwrap(),
            is_tcr,
            &barcode_data_sum,
            &mut json,
            single_end,
            &refdata,
            &inner_primersx,
            &outer_primersx,
            total_read_pairs,
            &mut report,
            is_gd,
        );
        println!("Done with metrics json");
        log_opts.report_perf_stats_now(&t, "after computing json metrics");
        drop(barcode_data_sum);
        let metrics_file: MetricsFile = rover.make_path("asm_metrics");
        let mut metrics_out = metrics_file.buf_writer()?;
        fwrite!(metrics_out, "{}", strme(&json));
        drop(metrics_out);

        // Validate json structure of metrics_summary_json.json.
        println!("Validate json structure");
        let json2 = read_to_string_safe(&metrics_file);
        let _: serde_json::Value = serde_json::from_str(&json2).unwrap_or_else(|_| {
            panic!(
                "{} is not well-formatted",
                metrics_file.as_ref().to_str().unwrap()
            )
        });

        println!("Validated");

        // Merge the two summaries
        let mut combined: JsonReporter = metrics_file.read()?;
        if let Some(ref_path) = args.vdj_reference_path {
            let mut ref_json_report: JsonReporter = JsonFile::new(ref_path, "reference").read()?;
            ref_json_report.add_prefix("vdj_reference");
            combined.merge(ref_json_report);
        }

        let combined_summary =
            MetricsFile::from_reporter(&rover, "metrics_summary_json", &combined)?;

        // Merge barcode support files.
        let barcode_support = {
            let path: CsvFile<()> = rover.make_path("barcode_support.csv");
            let mut writer = path.buf_writer()?;
            writeln!(&mut writer, "barcode,count")?;
            for co in &chunk_outs {
                let reader = co.outs_builder.lazy_reader()?;
                for builder in reader {
                    let builder: AssemblyOutsBuilder = builder?;
                    writeln!(&mut writer, "{},{}", builder.barcode, builder.umi_count)?;
                }
            }
            path
        };

        let barcode_data_brief_file: BarcodeDataBriefFile = rover.make_path("barcode_data_brief");
        barcode_data_brief_file.write(&barcode_data_brief)?;

        let assemblable_reads_per_bc = {
            let path: AsmReadsPerBcFormat = rover.make_path("assemblable_reads_per_bc");
            let mut hist = SimpleHistogram::new();
            for brief in barcode_data_brief {
                hist.insert(brief.barcode, brief.xucounts.iter().sum::<i32>());
            }
            path.write(&hist)?;
            path
        };

        // Return results.

        log_opts.report_perf_stats_now(&t, "before making barcodes_in_chunks");
        let barcodes_in_chunks = chunk_outs
            .iter()
            .map(|co| co.barcodes_in_chunk.clone())
            .collect();

        log_opts.report_perf_stats_now(&t, "join at Ok");
        // This is done so that pretty trace forgets about the full_file
        PrettyTrace::new().noexit().on();
        Ok(AssemblyStageOutputs {
            contig_bam: out_bam_filename,
            contig_bam_bai: contig_bam_bai_filename,
            summary_tsv: summary_tsv_file,
            umi_summary_tsv: umi_summary_tsv_file,
            metrics_summary_json: combined_summary,
            contig_annotations: contig_annotations_file,
            barcode_brief: barcode_data_brief_file,
            barcode_support,
            barcodes_in_chunks,
            assemblable_reads_per_bc,
            align_info: align_info_file,
            unmapped_sample_fastq: unmapped_sample_fastq_file,
            report: report_file,
        })
    }
}

fn load_refdata(
    vdj_reference_path: Option<&PathBuf>,
    is_tcr: bool,
    is_bcr: bool,
) -> (RefData, RefData, RefData, Vec<(Kmer20, i32, i32)>) {
    // Load reference and make a lookup table for it.  Actually there are three
    // versions:
    // (1) just for TCR or BCR (so long as we know which we have);
    // (2) for TCR and BCR, and with extra k=20 lookup table;
    // (3) just for TCR or BCR but also with extra sequences thrown in.

    let mut refdata = RefData::new();
    let mut refdatax = RefData::new();
    let mut refdata_full = RefData::new();
    let mut rkmers_plus_full_20 = Vec::<(Kmer20, i32, i32)>::new();
    if let Some(ref_path) = vdj_reference_path {
        let ref_path = ref_path.to_str().unwrap();
        let fasta_path = format!("{ref_path}/fasta/regions.fa");
        let fasta = read_to_string_safe(&fasta_path);
        let ext_fasta =
            fs::read_to_string(format!("{ref_path}/fasta/supp_regions.fa")).unwrap_or_default();
        if fasta.is_empty() {
            panic!("Reference file at {fasta_path} has zero length.");
        }
        make_vdj_ref_data_core(&mut refdata, &fasta, "", is_tcr, is_bcr, None);
        make_vdj_ref_data_core(&mut refdata_full, &fasta, "", true, true, None);
        make_kmer_lookup_20_single(&refdata_full.refs, &mut rkmers_plus_full_20);
        make_vdj_ref_data_core(&mut refdatax, &fasta, &ext_fasta, is_tcr, is_bcr, None);
    }
    (refdata, refdatax, refdata_full, rkmers_plus_full_20)
}

fn make_rtype(refdata_full: &RefData, has_refs: bool) -> Vec<i32> {
    if has_refs {
        refdata_full
            .rheaders
            .iter()
            .take(refdata_full.refs.len())
            .map(|header| {
                CHAIN_TYPESX
                    .iter()
                    .enumerate()
                    .filter_map(|(j, chain_type)| {
                        if header.contains(chain_type) {
                            Some(j as i32)
                        } else {
                            None
                        }
                    })
                    .last()
                    .unwrap_or(-1)
            })
            .collect()
    } else {
        Vec::<i32>::new()
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn write_simple_sam(
    args: &AssemblyStageInputs,
    split_args: &AssemblyChunkInputs,
    out_sam_filenamex: &Lz4File,
    contig_annotations_file: &JsonFile<Vec<ContigAnnotation>>,
    align_info_file: &TxtFile,
    umi_summary_file: &TsvFileNoHeader<()>,
    summary_file: &TsvFileNoHeader<()>,
    ob_file: &BincodeFile<Vec<AssemblyOutsBuilder>>,
    unmapped_sample_fastq_file: &FastqFile,
    refdata: &RefData,
    refdatax: RefData,
    refdata_full: RefData,
    inner_primersx: Vec<Vec<u8>>,
    outer_primersx: Vec<Vec<u8>>,
    rkmers_plus_full_20: Vec<(Kmer20, i32, i32)>,
    is_tcr: bool,
    is_bcr: bool,
    is_gd: Option<bool>,
    has_refs: bool,
    log_opts: &LogOpts,
    thread_message: &CHashMap<ThreadId, String>,
) -> Result<
    (
        bam::header::Header,
        Vec<String>,
        Vec<BarcodeData>,
        Vec<BarcodeDataBrief>,
    ),
    martian::Error,
> {
    let rtype = make_rtype(&refdata_full, has_refs);
    // Initialize heuristics.

    let mut heur = Heuristics::new();
    if args.denovo {
        heur.free = true;
    }
    // Null stuff.

    let lena = 0;
    let contam = FlowcellContam::new();
    let n50_n50_rpu = args.n50_n50_rpu;
    // Compute primer extensions.

    let inner_primer_exts = get_primer_exts(&inner_primersx, refdata);
    let outer_primer_exts = get_primer_exts(&outer_primersx, refdata);

    let mut log = Vec::<u8>::new();
    let mut ann_writer = contig_annotations_file.lazy_writer()?;
    let mut align_info = align_info_file.buf_writer()?;
    let mut umi_summary_writer = umi_summary_file.buf_writer()?;
    let mut summary_writer = summary_file.buf_writer()?;
    let mut ob_writer = ob_file.lazy_writer()?;
    let mut unmapped_sample_fastq = unmapped_sample_fastq_file.buf_writer()?;
    let vdj_adapters = crate::adapter::get_vdj_adapters();

    // Track cell barcodes, barcodes, and barcode data.

    let mut barcodes = Vec::<String>::new();
    let mut barcode_data = Vec::<BarcodeData>::new();
    let mut barcode_data_brief = Vec::<BarcodeDataBrief>::new();
    // Sam header.

    let mut sam_header = bam::header::Header::new();
    // Determine the id of this chunk.

    let ch = split_args.chunk_id;

    // Get number of read pairs.
    let npairs = args.npairs as usize;
    // Determine if single end.
    let single_end = !args.paired_end;
    // Define fraction of pairs to align to determine chain type.
    // target_frac = number out of 1000 to keep

    const TARGET_READS: usize = 1_000_000;
    let target_pairs = if single_end {
        TARGET_READS
    } else {
        TARGET_READS / 2
    };
    let target_frac = if target_pairs < npairs {
        (1000 * target_pairs) / npairs
    } else {
        1000
    };
    // Create sam writer.

    let mut simple_sam_writer = lz4::EncoderBuilder::new()
        .build(BufWriter::new(
            File::create(out_sam_filenamex).expect("could not open sam file for writing"),
        ))
        .expect("could not start building lz4");

    // Scope to force closure of simple_sam_writer2.

    {
        let barcode_counts_full =
            SimpleHistogram::<String>::from_file(&args.corrected_bc_counts, SerdeFormat::Json);
        let mut trimmer = crate::adapter::VdjTrimmer::new(&vdj_adapters);
        let mut simple_sam_writer2 = BufWriter::new(&mut simple_sam_writer);

        let bc_sorted_lazy_reader = split_args.chunk_rna_reads.lazy_reader()?;

        // Loop over all barcodes in the chunk.

        let (mut bid, mut rid) = (0, 0);
        let mut unmapped = 0;
        for (barcode, read_iter) in &bc_sorted_lazy_reader
            .group_by(|read: &Result<RnaRead>| read.as_ref().ok().map(HasBarcode::barcode))
        {
            let mut barcode_data_this = BarcodeData::new();

            let mut this_bc_reads = Vec::new();
            for rna_read in read_iter {
                let mut rna_read = rna_read?;
                trimmer.trim(&mut rna_read);
                this_bc_reads.push(rna_read);
            }
            let barcode = barcode.unwrap();
            // Vec<(umi, seq, qual, readname, flags)>
            let read_inner_data =
                crate::translator::make_read_data(&this_bc_reads, 8, args.r2_revcomp);
            // (barcode, Vec<(umi, seq, qual, readname, flags)>, actual reads)
            let barcode_string = barcode.to_string();
            let actual_reads = barcode_counts_full.get(&barcode_string);
            let read_data = (barcode_string, read_inner_data, actual_reads);
            barcode_data_this.nreads = read_data.2 as i32;
            // Assign fraction of reads used for assembly of each barcode
            let max_read_pairs_per_barcode = get_max_reads_per_barcode(args.paired_end);
            barcode_data_this.frac =
                if barcode_data_this.nreads as f64 <= max_read_pairs_per_barcode {
                    1.0_f64
                } else if barcode_data_this.nreads != 0 {
                    max_read_pairs_per_barcode / barcode_data_this.nreads as f64
                } else {
                    0.0_f64
                };

            // Set thread message.

            let chbid = format!("{ch}.{bid}");
            if thread::current().name().unwrap() == "main" {
                thread_message.insert(
                    thread::current().id(),
                    format!("while processing barcode {chbid} = {barcode}"),
                );
            }

            // Align a fixed fraction of the reads to determine their chain type.
            // This is annoying but it's expensive to align them all.

            if has_refs {
                for i in 0..read_data.1.len() {
                    if rid % 1000 < target_frac {
                        let b = read_data.1[i].1.clone(); // Read sequence
                        let mut best = chain_type(&b, &rkmers_plus_full_20, &rtype);
                        fwrite!(align_info, "{} ==> ", read_data.1[i].0); // UMI
                        if best >= 0 {
                            fwriteln!(align_info, "{}", OR_CHAIN_TYPES[best as usize]);
                        } else {
                            fwriteln!(align_info, "unmapped");
                            if unmapped % 50 == 0 {
                                fwriteln!(unmapped_sample_fastq, "@{}", read_data.1[i].3);
                                fwriteln!(
                                    unmapped_sample_fastq,
                                    "{}",
                                    read_data.1[i].1.to_string()
                                );
                                fwriteln!(unmapped_sample_fastq, "+");
                                let mut qual = read_data.1[i].2.clone();
                                for q in &mut qual {
                                    *q += 33;
                                }
                                fwriteln!(unmapped_sample_fastq, "{}", strme(&qual));
                            }
                            unmapped += 1;
                        }
                        if best == -1_i8 {
                            best = 14_i8;
                        }
                        barcode_data_this.chain_sample[best as usize] += 1;
                    }
                    rid += 1;
                }
            }
            unmapped_sample_fastq.flush()?;
            // Keep going.

            let barcode = read_data.0.clone();
            barcodes.push(barcode.clone());
            let t = Instant::now();
            // if track { println!( "START {}", chbid ); }
            let mut log2 = Vec::<u8>::new();

            fwriteln!(
                log2,
                "\n▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓\
                         ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓\n"
            );
            fwriteln!(log2, "BARCODE {} = {}\n", chbid, read_data.0);
            fwriteln!(log2, "using {} reads", read_data.1.len());
            barcode_data_this.barcode = read_data.0.clone();
            log_opts.report_perf_stats(&mut log2, &t, "before preprocessing reads");
            drop(read_data);

            let (corrected, umi_sorted_reads) =
                crate::translator::correct_umis(&mut this_bc_reads, args.r2_revcomp);
            let mut reads = umi_sorted_reads.reads;
            let mut quals = umi_sorted_reads.quals;
            let umi_id = umi_sorted_reads.umi_id;
            let uu = umi_sorted_reads.unique_umis;
            let flags = umi_sorted_reads.flags;
            let readnames = umi_sorted_reads.readnames;
            barcode_data_this.nreads_umi_corrected = corrected;

            // Create assemblies.

            let mut nedges = 0;
            let mut conx = Vec::<DnaString>::new();
            let mut conxq = Vec::<Vec<u8>>::new();
            let mut cids = Vec::<Vec<i32>>::new();
            let mut cumi = Vec::<Vec<i32>>::new();
            let mut productive = Vec::<bool>::new();
            let mut validated_umis = Vec::<Vec<String>>::new();
            let mut non_validated_umis = Vec::<Vec<String>>::new();
            let mut invalidated_umis = Vec::<Vec<String>>::new();
            let mut barcode_data_brief_this = BarcodeDataBrief::new();
            let mut junction_support = Vec::<Option<JunctionSupport>>::new();
            barcode_data_brief_this.barcode = barcode.clone();
            barcode_data_brief_this.read_pairs = barcode_data_this.nreads as u64;
            barcode_data_brief_this.frac_reads_used = barcode_data_this.frac;
            process_barcode(
                &chbid,
                single_end,
                is_tcr,
                is_bcr,
                is_gd,
                &inner_primersx,
                &outer_primersx,
                &inner_primer_exts,
                &outer_primer_exts,
                &mut reads,
                &mut quals,
                &umi_id,
                &uu,
                n50_n50_rpu as i32,
                refdata,
                &refdata_full,
                &rkmers_plus_full_20,
                &refdatax,
                lena,
                &contam,
                &mut barcode_data_this,
                &mut barcode_data_brief_this,
                &mut conx,
                &mut conxq,
                &mut productive,
                &mut validated_umis,
                &mut non_validated_umis,
                &mut invalidated_umis,
                &mut cids,
                &mut cumi,
                &mut nedges,
                &mut junction_support,
                &mut log2,
                log_opts,
                &heur,
            );
            log_opts.report_perf_stats(&mut log2, &t, "upon return from process_barcodes");

            // Write contigs and annotation for them.
            for i in 0..conx.len() {
                let tigname = format!("{barcode}_contig_{}", i + 1);
                // Build annotation.

                let can = ContigAnnotation::from_seq(
                    &conx[i],
                    &conxq[i],
                    &tigname,
                    refdata,
                    cids[i].len(),
                    cumi[i].len(),
                    false, // determined in ASM_CALL_CELLS stage
                    Some(validated_umis[i].clone()),
                    Some(non_validated_umis[i].clone()),
                    Some(invalidated_umis[i].clone()),
                    false, // determined in ASM_CALL_CELLS stage
                    is_gd,
                    junction_support[i].as_ref().cloned(),
                );

                // confirm that productive labels assigned to contigs
                // match up with assembler con and con2 piles.
                // this is not true for denovo mode
                if !heur.free {
                    assert_eq!(can.productive.unwrap(), productive[i]);
                }

                // Output as json with four space indentation
                ann_writer.write_item(&can)?;
            }
            log_opts.report_perf_stats(&mut log2, &t, "after writing contigs");

            // Make a vector showing which umis/reads are assigned to which contigs.
            // By design, each UMI is assigned to at most one contig, and each
            // read is assigned to at most one contig.
            let mut contig_of_read = vec![None; reads.len()];
            let mut contig_of_umi = match umi_id.last() {
                Some(&id) => vec![None; id as usize + 1],
                None => Vec::new(),
            };
            for (t, cid) in cids.iter().enumerate() {
                for &read_id in cid {
                    contig_of_read[read_id as usize] = Some(t);
                    contig_of_umi[umi_id[read_id as usize] as usize] = Some(t);
                }
            }

            // umi_count is the number of umis supporting the barcode.  There
            // are choices here as to how we count.  We use ALL the umis but
            // could change this.
            ob_writer.write_item(&AssemblyOutsBuilder {
                barcode: barcode.clone(),
                umi_count: barcode_data_this.xucounts.len(),
            })?;

            // Write umi_summary.  We do not enforce a lower bound on the number
            // of reads per UMI, and simply write "1" for the min_umi_reads
            // field, and declare every UMI good.
            for (umi, group) in &umi_id.iter().group_by(|&&id| id) {
                let tigname = contig_of_umi[umi as usize]
                    .map_or(String::new(), |t| format!("{barcode}_contig_{}", t + 1));
                const MIN_READS_PER_UMI: i32 = 1;
                fwriteln!(
                    umi_summary_writer,
                    "{}\t{}\t{}\t{}\t{}\ttrue\t{}",
                    barcode,
                    umi,
                    uu[umi as usize],
                    group.count(),
                    MIN_READS_PER_UMI,
                    tigname,
                );
            }
            umi_summary_writer.flush()?;

            // Write summary.
            for (t, cid) in cids.iter().enumerate() {
                let npairs = if single_end {
                    cid.len()
                } else {
                    // The following code is used to count the read
                    // pairs by checking whether both read1 and read2 are
                    // part of the reads in this contig. In paired end mode,
                    // the even ids are read1 and the adjacent odd ids are read 2.
                    cid.iter()
                        .tuple_windows()
                        .filter(|&(a, b)| *a % 2 == 0 && *b == *a + 1)
                        .count()
                };

                let umilist = cumi[t].iter().join(",");
                fwriteln!(
                    summary_writer,
                    "{}\t{}_contig_{}\t{}\t{}\t{}\t{}",
                    barcode,
                    barcode,
                    t + 1,
                    cid.len(),
                    npairs,
                    cumi[t].len(),
                    umilist
                );
            }
            summary_writer.flush()?;
            // Define scoring for alignments of reads to contigs.

            let scoring =
                sw::Scoring::from_scores(-GAP_OPEN, -GAP_EXTEND, MATCH_SCORE, -MISMATCH_SCORE)
                    .xclip(-CLIP)
                    .yclip(0);
            let min_align_score = 50.0;

            // ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓

            // Create a vector of Read objects.  We only fill in the entries
            // that are used below.
            // ◼ This is pretty horrible, as we're
            // ◼ just shoveling data into a different form.

            let mut readsx = Vec::<graph_read::Read>::new();
            for i in 0..reads.len() {
                let mut r = graph_read::Read::new(
                    i as ReadType,        // read id
                    0,                    // umi, "leaving blank"
                    readnames[i].clone(), // read name
                    reads[i].clone(),     // read sequence
                    quals[i].clone(),     // quality score for read
                );
                r.flags = flags[i]; // flags
                readsx.push(r);
            }
            log_opts.report_perf_stats(&mut log2, &t, "after creating readsx");

            // Add to the sam file.  First gather contig names.
            // ◼ The creation of bam headers here is gratuituous, since what
            // ◼ we put in there just gets regurgitated as sam records.  Since
            // ◼ the rust_htslib code is flaky, we should get rid of this
            // ◼ pointless conversion.

            let mut tignames = Vec::<String>::new();
            for (i, contig) in conx.iter().enumerate() {
                let contig_name = format!("{barcode}_contig_{}", i + 1);
                tignames.push(contig_name.clone());
                bam_utils::add_ref_to_bam_header(&mut sam_header, &contig_name, contig.len());
            }

            // ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
            // Create a Vec<Vec<u8>> from Vec<DnaString>
            let contig_seqs: Vec<_> = conx.iter().map(DnaString::to_ascii_vec).collect();
            let contig_aligners: Vec<_> = contig_seqs
                .iter()
                .map(|seq| {
                    ContigAligner::new(
                        seq,
                        scoring,
                        KMER_LEN_BANDED_ALIGN,
                        WINDOW_SIZE_BANDED_ALIGN,
                    )
                })
                .collect();
            let mut i = 0;
            while i < reads.len() {
                let read = reads[i].to_ascii_vec();
                let u = &uu[umi_id[i] as usize];
                let aln_packet = contig_of_read[i].and_then(|contig_id| {
                    contig_aligners[contig_id]
                        .align_read(&read, min_align_score as i32)
                        .map(|al| sw::AlignmentPacket {
                            ref_idx: contig_id,
                            alignment: al,
                        })
                });
                if single_end {
                    let rec = bam_utils::read_to_bam_record_opts(
                        &readsx[i],
                        &aln_packet,
                        &None,
                        true,
                        true,
                    );
                    write_sam_record_simple(&rec, u, &tignames, &mut simple_sam_writer2);
                    i += 1;
                } else {
                    let mate_read = reads[i + 1].to_ascii_vec();
                    let mate_aln_packet = contig_of_read[i + 1].and_then(|contig_id| {
                        contig_aligners[contig_id]
                            .align_read(&mate_read, min_align_score as i32)
                            .map(|al| sw::AlignmentPacket {
                                ref_idx: contig_id,
                                alignment: al,
                            })
                    });

                    let rec = bam_utils::read_to_bam_record_opts(
                        &readsx[i],
                        &aln_packet,
                        &mate_aln_packet,
                        true,
                        true,
                    );
                    let mate_rec = bam_utils::read_to_bam_record_opts(
                        &readsx[i + 1],
                        &mate_aln_packet,
                        &aln_packet,
                        true,
                        true,
                    );
                    write_sam_record_simple(&rec, u, &tignames, &mut simple_sam_writer2);
                    write_sam_record_simple(&mate_rec, u, &tignames, &mut simple_sam_writer2);

                    i += 2;
                }
            }
            // ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓

            log_opts.report_perf_stats(&mut log2, &t, "after simple sam");

            // Finish up.

            bid += 1;
            barcode_data.push(barcode_data_this);
            barcode_data_brief.push(barcode_data_brief_this);
            log_opts.report_perf_stats(&mut log2, &t, "after pushing result");
            if nedges > 0 || log_opts.keep_all {
                log.append(&mut log2);
            }
        }
    }

    // Print log.
    // ◼ Figure out if we want to print this and if so to _stdout or elsewhere.

    print!("{}", stringme(&log));

    // Finish writing of the simple_sam_writer.  It is not enough to have it go
    // out of scope, which seems like a bug in lz4.
    // Then end scope.  See commments at beginning of scope.

    let (_, res) = simple_sam_writer.finish();
    res?;
    ann_writer.finish()?;
    Ok((sam_header, barcodes, barcode_data, barcode_data_brief))
}

fn write_summary_tsv(summary: &TsvFile<()>, chunk_outs: &[AssemblyChunkOutputs]) -> Result<()> {
    let mut writer = summary.buf_writer()?;
    writeln!(
        &mut writer,
        "barcode\tcontig_name\tnum_reads\tnum_pairs\tnum_umis\tumi_list"
    )?;
    for co in chunk_outs {
        line_by_line_copy(&mut co.summary_tsv.buf_reader()?, &mut writer)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_umi_summary_tsv(
    umi_summary: &TsvFile<()>,
    chunk_outs: &[AssemblyChunkOutputs],
) -> Result<()> {
    let mut writer = umi_summary.buf_writer()?;
    writeln!(
        &mut writer,
        "barcode\tumi_id\tumi\treads\tmin_umi_reads\tgood_umi\tcontigs"
    )?;
    for co in chunk_outs {
        line_by_line_copy(&mut co.umi_summary_tsv.buf_reader()?, &mut writer)?;
    }
    // Flush so that any errors are raised here
    writer.flush()?;
    Ok(())
}
