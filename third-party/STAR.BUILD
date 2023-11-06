# This is for the 2.7.2a-tenx version of STAR

load("@rules_cc//cc:defs.bzl", "cc_binary", "cc_library")
load(
    "@rules_license//rules:license.bzl",
    "license",
)

package(
    default_applicable_licenses = ["license"],
    features = ["thin_lto"],
)

license(
    name = "license",
    package_name = "STAR",
    additional_info = {
        "homepage": "https://github.com/alexdobin/STAR",
        "version": "2.7.2a-tenx",
        "manifest": "third-party/deps.bzl",
        "pURL": "pkg:github/10XGenomics/STAR@2.7.2a-tenx",
    },
    copyright_notice = "Copyright (c) 2019 Alexander Dobin",
    license_kinds = [
        "@rules_license//licenses/spdx:MIT",
    ],
    license_text = "LICENSE",
)

cc_binary(
    name = "STAR",
    srcs = [
        "source/STAR.cpp",
    ],
    copts = [
        "-Wno-literal-conversion",
    ],
    visibility = ["//visibility:public"],
    deps = [
        ":star",
    ],
)

genrule(
    name = "parametersDefault",
    srcs = ["source/parametersDefault"],
    outs = ["source/parametersDefault.xxd"],
    cmd = "(cd $$(dirname $<) && xxd -i $$(basename $<)) > $@",
)

cc_library(
    name = "star",
    srcs = [
        "source/BAMbinSortByCoordinate.cpp",
        "source/BAMbinSortUnmapped.cpp",
        "source/BAMfunctions.cpp",
        "source/BAMoutput.cpp",
        "source/Chain.cpp",
        "source/ChimericAlign.cpp",
        "source/ChimericAlign_chimericJunctionOutput.cpp",
        "source/ChimericAlign_chimericStitching.cpp",
        "source/ChimericDetection.cpp",
        "source/ChimericDetection_chimericDetectionMult.cpp",
        "source/ChimericSegment.cpp",
        "source/ErrorWarning.cpp",
        "source/Genome.cpp",
        "source/Genome_genomeGenerate.cpp",
        "source/Genome_insertSequences.cpp",
        "source/GlobalVariables.cpp",
        "source/InOutStreams.cpp",
        "source/OutSJ.cpp",
        "source/PackedArray.cpp",
        "source/Parameters.cpp",
        "source/ParametersChimeric_initialize.cpp",
        "source/ParametersSolo.cpp",
        "source/Parameters_closeReadsFiles.cpp",
        "source/Parameters_openReadsFiles.cpp",
        "source/Parameters_readSAMheader.cpp",
        "source/Quantifications.cpp",
        "source/ReadAlign.cpp",
        "source/ReadAlignChunk.cpp",
        "source/ReadAlignChunk_mapChunk.cpp",
        "source/ReadAlignChunk_processChunks.cpp",
        "source/ReadAlign_alignBAM.cpp",
        "source/ReadAlign_assignAlignToWindow.cpp",
        "source/ReadAlign_calcCIGAR.cpp",
        "source/ReadAlign_chimericDetection.cpp",
        "source/ReadAlign_chimericDetectionOld.cpp",
        "source/ReadAlign_chimericDetectionOldOutput.cpp",
        "source/ReadAlign_chimericDetectionPEmerged.cpp",
        "source/ReadAlign_createExtendWindowsWithAlign.cpp",
        "source/ReadAlign_mapOneRead.cpp",
        "source/ReadAlign_mappedFilter.cpp",
        "source/ReadAlign_maxMappableLength2strands.cpp",
        "source/ReadAlign_multMapSelect.cpp",
        "source/ReadAlign_oneRead.cpp",
        "source/ReadAlign_outputAlignments.cpp",
        "source/ReadAlign_outputTranscriptCIGARp.cpp",
        "source/ReadAlign_outputTranscriptSAM.cpp",
        "source/ReadAlign_outputTranscriptSJ.cpp",
        "source/ReadAlign_peOverlapMergeMap.cpp",
        "source/ReadAlign_quantTranscriptome.cpp",
        "source/ReadAlign_stitchPieces.cpp",
        "source/ReadAlign_stitchWindowSeeds.cpp",
        "source/ReadAlign_storeAligns.cpp",
        "source/ReadAlign_waspMap.cpp",
        "source/SequenceFuns.cpp",
        "source/SharedMemory.cpp",
        "source/Solo.cpp",
        "source/SoloFeature.cpp",
        "source/SoloFeature_collapseUMI.cpp",
        "source/SoloFeature_outputResults.cpp",
        "source/SoloFeature_processRecords.cpp",
        "source/SoloRead.cpp",
        "source/SoloReadBarcode.cpp",
        "source/SoloReadBarcode_getCBandUMI.cpp",
        "source/SoloReadFeature.cpp",
        "source/SoloReadFeature_inputRecords.cpp",
        "source/SoloReadFeature_record.cpp",
        "source/SoloRead_record.cpp",
        "source/Stats.cpp",
        "source/SuffixArrayFuns.cpp",
        "source/ThreadControl.cpp",
        "source/TimeFunctions.cpp",
        "source/Transcript.cpp",
        "source/Transcript_alignScore.cpp",
        "source/Transcript_generateCigarP.cpp",
        "source/Transcript_variationAdjust.cpp",
        "source/Transcriptome.cpp",
        "source/Transcriptome_geneCountsAddAlign.cpp",
        "source/Transcriptome_geneFullAlignOverlap.cpp",
        "source/Transcriptome_quantAlign.cpp",
        "source/Variation.cpp",
        "source/alignSmithWaterman.cpp",
        "source/bamRemoveDuplicates.cpp",
        "source/bam_cat.cpp",
        "source/binarySearch2.cpp",
        "source/blocksOverlap.cpp",
        "source/extendAlign.cpp",
        "source/funCompareUintAndSuffixes.cpp",
        "source/funCompareUintAndSuffixesMemcmp.cpp",
        "source/genomeParametersWrite.cpp",
        "source/genomeSAindex.cpp",
        "source/genomeScanFastaFiles.cpp",
        "source/insertSeqSA.cpp",
        "source/loadGTF.cpp",
        "source/mapThreadsSpawn.cpp",
        "source/outputSJ.cpp",
        "source/readLoad.cpp",
        "source/signalFromBAM.cpp",
        "source/sjdbBuildIndex.cpp",
        "source/sjdbInsertJunctions.cpp",
        "source/sjdbLoadFromFiles.cpp",
        "source/sjdbLoadFromStream.cpp",
        "source/sjdbPrepare.cpp",
        "source/stitchAlignToTranscript.cpp",
        "source/stitchWindowAligns.cpp",
        "source/streamFuns.cpp",
        "source/stringSubstituteAll.cpp",
        "source/sysRemoveDir.cpp",
    ],
    hdrs = glob(["source/*.h"]) + [
        "source/VERSION",
        "source/serviceFuns.cpp",
        "source/sjAlignSplit.cpp",
        ":parametersDefault",
    ],
    copts = [
        "-fopenmp",
        "-std=c++14",
        "-D'COMPILATION_TIME_PLACE=\"__REDACTED__\"'",
        "-Wno-unused-but-set-variable",
        "-Wno-unused-function",
        "-Wno-unused-private-field",
        "-Wno-unqualified-std-cast-call",
        "-Wno-deprecated-register",
    ],
    includes = [
        "source",
    ],
    linkopts = [
        "-pthread",
        "-lm",
        "-fopenmp",
    ],
    linkstatic = 1,
    deps = [
        ":hts",
        "@zlib",
    ],
)

cc_library(
    name = "hts",
    srcs = [
        "source/htslib/bgzf.c",
        "source/htslib/cram/cram_codecs.c",
        "source/htslib/cram/cram_decode.c",
        "source/htslib/cram/cram_encode.c",
        "source/htslib/cram/cram_index.c",
        "source/htslib/cram/cram_io.c",
        "source/htslib/cram/cram_samtools.c",
        "source/htslib/cram/cram_stats.c",
        "source/htslib/cram/files.c",
        "source/htslib/cram/mFILE.c",
        "source/htslib/cram/md5.c",
        "source/htslib/cram/open_trace_file.c",
        "source/htslib/cram/pooled_alloc.c",
        "source/htslib/cram/sam_header.c",
        "source/htslib/cram/string_alloc.c",
        "source/htslib/cram/thread_pool.c",
        "source/htslib/cram/vlen.c",
        "source/htslib/cram/zfio.c",
        "source/htslib/faidx.c",
        "source/htslib/hfile.c",
        "source/htslib/hfile_net.c",
        "source/htslib/hts.c",
        "source/htslib/kfunc.c",
        "source/htslib/knetfile.c",
        "source/htslib/kstring.c",
        "source/htslib/sam.c",
        "source/htslib/synced_bcf_reader.c",
        "source/htslib/tbx.c",
        "source/htslib/vcf.c",
        "source/htslib/vcf_sweep.c",
        "source/htslib/vcfutils.c",
    ],
    hdrs = glob([
        "source/htslib/*.h",
        "source/htslib/cram/*.h",
        "source/htslib/htslib/*.h",
    ]),
    copts = [
        "-DSAMTOOLS=1",
        "-Wno-misleading-indentation",
        "-Wno-unused-but-set-variable",
        "-Wno-unused-function",
        "-Wno-unused-variable",
    ],
    includes = [
        "source/htslib",
    ],
    linkstatic = 1,
)
