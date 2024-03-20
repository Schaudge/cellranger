use crate::binned::SquareBinRowOrColumnIndex;
use crate::{
    io_utils, BarcodeConstruct, BarcodeSegment, BarcodeSegmentContent, BcSegSeq,
    GelBeadAndProbeConstruct,
};
use anyhow::{bail, Context, Result};
use barcodes_folder::find_whitelist;
pub use barcodes_folder::{find_atac_whitelist, find_slide_design};
use itertools::{process_results, Itertools};
use martian::{AsMartianPrimaryType, MartianStruct};
use martian_derive::MartianStruct;
use metric::{TxHashMap, TxHashSet};
use serde::{Deserialize, Serialize};
use slide_design::{load_oligos, spot_pitch, OligoPart};
use std::collections::HashSet;
use std::io::BufRead;
use std::ops::Range;
use std::path::{Path, PathBuf};
use strum_macros::Display;

/// Different ways to specify a whitelist of sequences (typically barcode sequence)
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(into = "WhitelistSpecFlat", try_from = "WhitelistSpecFlat")]
#[serde(untagged)]
pub enum WhitelistSpec {
    // NOTE: if you add, remove, or alter fields here, you MUST update the
    // fields contained in the WhitelistSpecFlat helper struct below.
    /// Historically we have specified barcode whitelists using a name. A txt file with that name
    /// would be located in the `lib/python/cellranger/barcodes` folder.
    TxtFile {
        name: String,
    },
    /// A translation whitelist dynamically generated by the pipeline.
    DynamicTranslation {
        translation_whitelist_path: PathBuf,
    },
    SlideFile {
        slide: String,
        part: OligoPart,
    },
}

impl WhitelistSpec {
    pub fn whitelist_name(&self) -> Option<&str> {
        if let WhitelistSpec::TxtFile { name } = self {
            Some(name)
        } else {
            None
        }
    }

    pub fn slide_name(&self) -> Option<&str> {
        if let WhitelistSpec::SlideFile { slide, .. } = self {
            Some(slide)
        } else {
            None
        }
    }

    /// Produce the whitelist loader corresponding to this spec.
    pub fn as_source(&self, translation: bool) -> Result<WhitelistSource> {
        Ok(match self {
            WhitelistSpec::TxtFile { name } => WhitelistSource::TxtFile {
                path: find_whitelist(name, translation)?,
            },
            WhitelistSpec::DynamicTranslation {
                translation_whitelist_path: whitelist_path,
            } => WhitelistSource::DynamicTranslation {
                path: whitelist_path.clone(),
            },
            WhitelistSpec::SlideFile { slide, part } => WhitelistSource::SlideFile {
                path: find_whitelist(slide, translation)?,
                part: *part,
            },
        })
    }
}

impl AsMartianPrimaryType for WhitelistSpec {
    fn as_martian_primary_type() -> martian::MartianPrimaryType {
        martian::MartianPrimaryType::Struct(martian::StructDef::new(
            "WhitelistSpec".to_string(),
            WhitelistSpecFlat::mro_fields(),
        ))
    }
}

/// Helper struct to flatten a WhitelistSpec into a martian struct.
#[derive(Serialize, Deserialize, Default, MartianStruct)]
struct WhitelistSpecFlat {
    name: Option<String>,
    #[mro_type = "file"]
    translation_whitelist_path: Option<PathBuf>,
    slide: Option<String>,
    part: Option<OligoPart>,
}

impl From<WhitelistSpec> for WhitelistSpecFlat {
    fn from(value: WhitelistSpec) -> Self {
        match value {
            WhitelistSpec::TxtFile { name } => Self {
                name: Some(name),
                ..Self::default()
            },
            WhitelistSpec::DynamicTranslation {
                translation_whitelist_path,
            } => Self {
                translation_whitelist_path: Some(translation_whitelist_path),
                ..Self::default()
            },
            WhitelistSpec::SlideFile { slide, part } => Self {
                slide: Some(slide),
                part: Some(part),
                ..Self::default()
            },
        }
    }
}

impl TryFrom<WhitelistSpecFlat> for WhitelistSpec {
    type Error = anyhow::Error;

    fn try_from(value: WhitelistSpecFlat) -> Result<Self> {
        match (
            value.name,
            value.translation_whitelist_path,
            value.slide,
            value.part,
        ) {
            (Some(name), None, None, None) => Ok(Self::TxtFile { name }),
            (None, Some(path), None, None) => Ok(Self::DynamicTranslation {
                translation_whitelist_path: path,
            }),
            (None, None, Some(slide), Some(part)) => Ok(Self::SlideFile { slide, part }),
            too_many => bail!("too many fields set in whitelist spec: {:?}", too_many),
        }
    }
}

/// Should mirror `WhitelistSpec`.
/// `WhitelistSource` would have the full path to the apropriate files that
/// contains the whitelist sequences. There are convenience functions to access
/// the whitelist as a set/vec/map etc.
#[derive(Debug, Eq, PartialEq)]
pub enum WhitelistSource {
    /// A headerless TSV file of barcode sequences, optional translated barcode sequences, and optional identifier.
    TxtFile {
        path: PathBuf,
    },
    /// A translation whitelist, always contains two sequences.
    DynamicTranslation {
        path: PathBuf,
    },
    SlideFile {
        path: PathBuf,
        part: OligoPart,
    },
}

/// The identifier given to a particular barcode or collection of related barcodes.
pub type BarcodeId = crate::ShortString7;

/// The known types of multiplexing barcode identifiers.
#[derive(Debug, Copy, Clone, Display, PartialEq, Eq)]
pub enum MultiplexingBarcodeType {
    CMO,
    #[strum(to_string = "Gene Expression (BC)")]
    RTL,
    #[strum(to_string = "Antibody Capture (AB)")]
    Antibody,
    #[strum(to_string = "CRISPR Guide Capture (CR)")]
    Crispr,
    Overhang,
}

/// Categorize the provided multiplexing barcode ID by type.
/// Assume that any unrecognized identifier is a (potentially user-provided) CMO ID.
/// BC001-BC024 are RTL multiplexing barcodes.
/// BC025-BC999 and ABxxx are antibody multiplexing barcodes.
/// CRxxx are CRISPR guide multiplexing barcodes.
pub fn categorize_multiplexing_barcode_id(bc_id: &str) -> MultiplexingBarcodeType {
    #[allow(clippy::enum_glob_use)]
    use MultiplexingBarcodeType::*;
    match (&bc_id[..2], bc_id[2..].parse::<usize>().ok()) {
        ("BC", Some(..=24)) => RTL,
        ("BC", None) if matches!(bc_id.as_bytes().last(), Some(b'A'..=b'D')) => RTL,
        ("BC", Some(25..)) | ("AB", Some(_)) => Antibody,
        ("CR", Some(_)) => Crispr,
        ("OH", Some(_)) => Overhang,
        _ => CMO,
    }
}

type Entry = (BcSegSeq, Option<BcSegSeq>, Option<BarcodeId>);

impl WhitelistSource {
    pub fn named(name: &str, translation: bool) -> Result<Self> {
        Ok(Self::TxtFile {
            path: find_whitelist(name, translation)?,
        })
    }

    pub fn txt_file(path: &Path) -> Self {
        WhitelistSource::TxtFile {
            path: path.to_path_buf(),
        }
    }

    pub fn construct(
        spec: BarcodeConstruct<&WhitelistSpec>,
        gel_bead_translation: bool,
    ) -> Result<BarcodeConstruct<Self>> {
        let translation = match spec {
            BarcodeConstruct::GelBeadOnly(_) => BarcodeConstruct::GelBeadOnly(gel_bead_translation),
            BarcodeConstruct::GelBeadAndProbe(_) => {
                assert!(!gel_bead_translation);
                // A given pool of barcoded RTL probes are a mix of four barcodes to base balance
                // in sequencing. We map the four barcodes in the mix to a single barcode using
                // translation machinery
                BarcodeConstruct::GelBeadAndProbe(GelBeadAndProbeConstruct {
                    gel_bead: false,
                    probe: true,
                })
            }
            BarcodeConstruct::Segmented(_) => {
                assert!(!gel_bead_translation);
                spec.map(|_| false)
            }
        };

        spec.zip(translation).map_result(|(s, t)| s.as_source(t))
    }

    fn path(&self) -> &Path {
        match self {
            Self::TxtFile { path } => path,
            Self::DynamicTranslation { path } => path,
            Self::SlideFile { path, .. } => path,
        }
    }

    fn is_translation(&self) -> bool {
        match self {
            WhitelistSource::TxtFile { path } => {
                // if the parent directory is "translation"
                path.parent()
                    .and_then(|p| p.file_name().map(|d| d == "translation"))
                    .unwrap_or(false)
            }
            WhitelistSource::DynamicTranslation { .. } => true,
            WhitelistSource::SlideFile { .. } => false,
        }
    }

    pub fn iter(&self) -> Result<Box<dyn Iterator<Item = Result<Entry>> + '_>> {
        let iter = match self {
            WhitelistSource::TxtFile { path } | WhitelistSource::DynamicTranslation { path } => {
                Box::new(
                    io_utils::open_with_gz(path)
                        .with_context(|| "whitelist file not found")?
                        .lines()
                        .map(|line| line.with_context(|| path.display().to_string()))
                        .map_ok(|line| {
                            // could be a translation whitelist, take left only
                            let mut iter = line.split_whitespace();
                            let lhs = BcSegSeq::from_bytes(iter.next().unwrap().as_bytes());
                            let rhs = iter.next().map(|x| BcSegSeq::from_bytes(x.as_bytes()));
                            let id = iter.next().map(|id| BarcodeId::try_from(id).unwrap());
                            (lhs, rhs, id)
                        }),
                ) as Box<dyn Iterator<Item = Result<Entry>>>
            }
            WhitelistSource::SlideFile { path, part } => Box::new(
                load_oligos(path, *part)?
                    .into_iter()
                    .map(|s| Ok((BcSegSeq::from_bytes(s.as_bytes()), None, None))),
            ),
        };
        Ok(iter)
    }

    /// Read the barcode whitelist and return a vector of BcSegSeq.
    pub fn as_vec(&self) -> Result<Vec<BcSegSeq>> {
        self.iter()?.map_ok(|x| x.0).collect()
    }

    /// Read the barcode whitelist and return a set of BcSegSeq.
    pub fn as_set(&self) -> Result<TxHashSet<BcSegSeq>> {
        self.iter()?.map_ok(|x| x.0).collect()
    }

    /// Read the barcode whitelist and return a map of BCsegSeq to integers.
    pub fn as_map(&self) -> Result<TxHashMap<BcSegSeq, u32>> {
        self.iter()?
            .enumerate()
            .map(|(i, x)| x.map(|x| (x.0, i as u32)))
            .collect()
    }

    /// Read the barcode (translation) whitelist and return a map of BCsegSeq to BcSegSeq.
    pub fn as_translation(&self) -> Result<TxHashMap<BcSegSeq, BcSegSeq>> {
        self.iter()?
            .map(|x| {
                if let (raw_seq, Some(trans_seq), _) = x? {
                    Ok((raw_seq, trans_seq))
                } else {
                    bail!("not a translation whitelist: {self:?}")
                }
            })
            .collect()
    }

    pub fn as_whitelist(&self) -> Result<Whitelist> {
        if let WhitelistSource::SlideFile { path, part } = self {
            let spot_pitch = spot_pitch(path)?;
            return Ok(Whitelist::SpatialHd(
                load_oligos(path, *part)?
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        (
                            BcSegSeq::from_bytes(s.as_bytes()),
                            SquareBinRowOrColumnIndex {
                                index: i,
                                size_um: spot_pitch,
                            },
                        )
                    })
                    .collect(),
            ));
        }
        if self.is_translation() {
            Ok(Whitelist::Trans(self.as_translation()?))
        } else {
            Ok(Whitelist::Plain(self.as_set()?))
        }
    }

    pub fn as_translation_seq_to_id(&self) -> Result<TxHashMap<BcSegSeq, BarcodeId>> {
        self.iter()?
            .map(|x| match x? {
                (_raw_seq, Some(trans_seq), Some(id)) => Ok((trans_seq, id)),
                (_, None, _) | (_, _, None) => {
                    bail!("Barcode translation whitelist requires three columns")
                }
            })
            .collect()
    }

    fn as_id_to_translation_seq(&self) -> Result<TxHashMap<BarcodeId, BcSegSeq>> {
        let mut id_to_translated_seq: TxHashMap<BarcodeId, BcSegSeq> = TxHashMap::default();
        for entry in self.iter()? {
            let (_raw_seq, trans_seq, id) = entry?;
            let (trans_seq, id) = (trans_seq.unwrap(), id.unwrap());
            let existing = id_to_translated_seq.entry(id).or_insert(trans_seq);
            assert_eq!(
                *existing, trans_seq,
                "found multiple translated sequences for barcode {id}: {existing} {trans_seq}",
            );
        }
        Ok(id_to_translated_seq)
    }

    pub fn as_raw_seq_to_id(&self) -> Result<TxHashMap<BcSegSeq, BarcodeId>> {
        self.iter()?
            .map(|x| match x? {
                (raw_seq, Some(_trans_seq), Some(id)) => Ok((raw_seq, id)),
                (_, None, _) | (_, _, None) => {
                    bail!("Barcode translation whitelist requires three columns")
                }
            })
            .collect()
    }

    /// Return a sorted vector of the barcode IDs.
    pub fn get_ids(&self) -> Result<Vec<BarcodeId>> {
        process_results(self.iter()?, |iter| {
            iter.map(|x| x.2.unwrap()).sorted().dedup().collect()
        })
    }

    /// Create a translation whitelist from this whitelist and provided map, as
    /// well as the whitelist containing the target barcodes to be translated
    /// into.
    ///
    /// The output whitelist will always be a translation-style whitelist.
    /// All barcodes provided in the ID mapping will be mapped to the target
    /// barcode. All barcodes not involved in a translation pair
    /// will be included verbatim in the output whitelist.
    /// More than one barcode may be mapped to the same target.
    ///
    /// Note that this implementation is not optimized for very large whitelists
    /// and may need to be refactored for such a use case.
    pub fn create_translation_from_id_map<'a>(
        &'a self,
        id_map: &'a TxHashMap<BarcodeId, BarcodeId>,
        translate_to: &'a Self,
    ) -> Result<impl Iterator<Item = (BcSegSeq, BcSegSeq, BarcodeId)> + 'a> {
        assert!(
            self.is_translation(),
            "cannot create a mapped translation of whitelist {} because it itself is not a translation whitelist",
            self.path().display()
        );
        assert!(
            translate_to.is_translation(),
            "cannot create a mapped translation of whitelist {} to whitelist {} because the latter is not a translation whitelist",
            self.path().display(),
            translate_to.path().display()
        );

        // Generate a combined mapping of ID to translated sequence for this and
        // the target whitelist.
        let id_to_translated_seq = {
            let mut this_id_to_translated_seq = self.as_id_to_translation_seq()?;
            let target_id_to_translated_seq = translate_to.as_id_to_translation_seq()?;
            if self != translate_to {
                // The only case in which we should have any overlap here is if the
                // two input whitelists are identical.
                let this_ids: HashSet<_> = this_id_to_translated_seq.keys().collect();
                let target_ids: HashSet<_> = target_id_to_translated_seq.keys().collect();
                let id_overlap: Vec<_> = this_ids.intersection(&target_ids).collect();
                assert!(id_overlap.is_empty(), "some probe barcode IDs were found in both the translated and target whitelists: {id_overlap:?}");
            }

            this_id_to_translated_seq.extend(target_id_to_translated_seq);
            this_id_to_translated_seq
        };

        // Load all of the entries in this whitelist.
        let entries: Vec<_> = self
            .iter()?
            .map(|entry| entry.map(|(raw_seq, _trans_seq, id)| (raw_seq, id.unwrap())))
            .try_collect()?;

        // Now use our two mappings to translate the whitelist.  Maintain
        // the order of the original whitelist to ease debugging.
        Ok(entries.into_iter().map(move |(raw_seq, original_id)| {
            let translated_id = id_map.get(&original_id).unwrap_or(&original_id);

            let new_translated_seq = id_to_translated_seq.get(translated_id).unwrap_or_else(|| {
                panic!(
                    "{translated_id} not found in combined whitelists {} and {}",
                    self.path().display(),
                    translate_to.path().display(),
                )
            });
            (raw_seq, *new_translated_seq, *translated_id)
        }))
    }
}

#[derive(Serialize)]
pub enum Whitelist {
    Plain(TxHashSet<BcSegSeq>),
    Trans(TxHashMap<BcSegSeq, BcSegSeq>),
    SpatialHd(TxHashMap<BcSegSeq, SquareBinRowOrColumnIndex>),
}

impl Whitelist {
    pub fn new(path: &Path) -> Result<Self> {
        WhitelistSource::txt_file(path).as_whitelist()
    }

    // Range of length of sequences in this whitelist
    pub fn sequence_lengths(&self) -> Range<usize> {
        let lengths: TxHashSet<_> = match self {
            Whitelist::Plain(ref seqs) => seqs.iter().map(BcSegSeq::len).collect(),
            Whitelist::Trans(ref seqs) => seqs
                .iter()
                .flat_map(|(s0, s1)| [s0.len(), s1.len()])
                .collect(),
            Whitelist::SpatialHd(ref seqs) => seqs.keys().map(BcSegSeq::len).collect(),
        };
        lengths
            .into_iter()
            .minmax()
            .into_option()
            .map_or(0..0, |(min, max)| min..max + 1)
    }

    pub fn construct(
        spec: BarcodeConstruct<&WhitelistSpec>,
        gel_bead_translation: bool,
    ) -> Result<BarcodeConstruct<Self>> {
        WhitelistSource::construct(spec, gel_bead_translation)?.map_result(|x| x.as_whitelist())
    }

    /// Given a barcode segment, check if it is in the whitelist and update the content.
    ///
    /// - If the whitelist is a translation whitelist, update the barcode segment's content
    /// to the translated sequence if there is a match.
    /// - If the whitelist is a spatial HD whitelist, update the barcode segment's content
    /// to the spatial index if there is a match.
    pub fn check_and_update(&self, bc_segment: &mut BarcodeSegment) -> bool {
        let seq_in_wl = match self {
            Whitelist::Plain(ref whitelist) => whitelist.contains(bc_segment.sequence()),
            Whitelist::Trans(ref whitelist) => {
                if let Some(translation) = whitelist.get(bc_segment.sequence()) {
                    bc_segment.content = BarcodeSegmentContent::Sequence(*translation);
                    true
                } else {
                    false
                }
            }
            Whitelist::SpatialHd(whitelist) => {
                if let Some(index) = whitelist.get(bc_segment.sequence()) {
                    bc_segment.content =
                        BarcodeSegmentContent::SpatialIndex(*bc_segment.sequence(), *index);
                    true
                } else {
                    false
                }
            }
        };
        bc_segment.state.change(seq_in_wl);
        seq_in_wl
    }

    pub fn contains(&self, sequence: &BcSegSeq) -> bool {
        match self {
            Whitelist::Plain(ref whitelist) => whitelist.contains(sequence),
            Whitelist::Trans(ref whitelist) => whitelist.contains_key(sequence),
            Whitelist::SpatialHd(ref whitelist) => whitelist.contains_key(sequence),
        }
    }

    // - Return `sseq` if it is in the whitelist
    // - If there's a single N in the barcode, check if we can can replace the N with a valid base &
    // get a whitelist hit. This makes us robust to N-cycles. Return the first modified sequence if
    // we found one.
    // - Otherwise return None
    pub fn match_to_whitelist(&self, mut sseq: BcSegSeq) -> Option<BcSegSeq> {
        if self.contains(&sseq) {
            return Some(sseq);
        }

        let Some(pos_n) = sseq.seq().iter().position(|&base| base == b'N') else {
            return None;
        };

        b"ACGT".iter().find_map(|&base| {
            sseq[pos_n] = base;
            self.contains(&sseq).then_some(sseq)
        })
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use std::io::{BufWriter, Write};

    #[test]
    fn test_match_to_whitelist() {
        use metric::set;
        let wl = Whitelist::Plain(set![BcSegSeq::from_bytes(b"ACGT")]);
        assert_eq!(
            wl.match_to_whitelist(BcSegSeq::from_bytes(b"ACGT")),
            Some(BcSegSeq::from_bytes(b"ACGT"))
        );
        assert_eq!(wl.match_to_whitelist(BcSegSeq::from_bytes(b"AAAT")), None);
        assert_eq!(
            wl.match_to_whitelist(BcSegSeq::from_bytes(b"ANGT")),
            Some(BcSegSeq::from_bytes(b"ACGT"))
        );
    }

    #[test]
    fn test_whitelist_spec_serde() -> Result<()> {
        let specs = vec![
            WhitelistSpec::TxtFile {
                name: "foo".to_string(),
            },
            WhitelistSpec::DynamicTranslation {
                translation_whitelist_path: PathBuf::from("/test"),
            },
            WhitelistSpec::SlideFile {
                slide: "test_slide".to_string(),
                part: OligoPart::Bc1,
            },
        ];
        let de: Vec<WhitelistSpec> = serde_json::from_str(&serde_json::to_string(&specs)?)?;
        assert_eq!(de, specs);
        Ok(())
    }

    #[test]
    fn test_create_translation_from_overlapping_id_map() -> Result<()> {
        let (source, _file) = create_translation_whitelist(vec![
            vec!["ACTG", "AAAA", "BC0"],
            vec!["ATCG", "AAAA", "BC0"],
            vec!["CTAG", "TTTT", "BC1"],
            vec!["TACG", "CCCC", "BC2"],
            vec!["TAAG", "CCCC", "BC2"],
            vec!["TCAG", "GGGG", "BC3"],
            vec!["GACT", "CCCC", "BC4"],
            vec!["GCAT", "GGCC", "BC5"],
        ]);
        let mapping: TxHashMap<BarcodeId, BarcodeId> =
            [("BC2", "BC0"), ("BC3", "BC0"), ("BC4", "BC1")]
                .into_iter()
                .map(|(id0, id1)| {
                    (
                        BarcodeId::try_from(id0).unwrap(),
                        BarcodeId::try_from(id1).unwrap(),
                    )
                })
                .collect();

        let translated: Vec<_> = source
            .create_translation_from_id_map(&mapping, &source)?
            .map(|(s0, s1, id)| vec![s0.to_string(), s1.to_string(), id.to_string()])
            .collect();

        let expected = owned_string_vecs(vec![
            vec!["ACTG", "AAAA", "BC0"],
            vec!["ATCG", "AAAA", "BC0"],
            vec!["CTAG", "TTTT", "BC1"],
            vec!["TACG", "AAAA", "BC0"],
            vec!["TAAG", "AAAA", "BC0"],
            vec!["TCAG", "AAAA", "BC0"],
            vec!["GACT", "TTTT", "BC1"],
            vec!["GCAT", "GGCC", "BC5"],
        ]);

        assert_eq!(translated, expected);

        Ok(())
    }

    #[test]
    fn test_create_translation_from_non_overlapping_id_maps() -> Result<()> {
        let (target, _target_file) = create_translation_whitelist(vec![
            vec!["ACTG", "AAAA", "BC0"],
            vec!["ATCG", "AAAA", "BC0"],
            vec!["CTAG", "TTTT", "BC1"],
        ]);
        let (source, _source_file) = create_translation_whitelist(vec![
            vec!["TACG", "CCCC", "BC2"],
            vec!["TAAG", "CCCC", "BC2"],
            vec!["TCAG", "GGGG", "BC3"],
            vec!["GACT", "CCCC", "BC4"],
            vec!["GCAT", "GGCC", "BC5"],
        ]);
        let mapping: TxHashMap<BarcodeId, BarcodeId> =
            [("BC2", "BC0"), ("BC3", "BC0"), ("BC4", "BC1")]
                .into_iter()
                .map(|(id0, id1)| {
                    (
                        BarcodeId::try_from(id0).unwrap(),
                        BarcodeId::try_from(id1).unwrap(),
                    )
                })
                .collect();

        let translated: Vec<_> = source
            .create_translation_from_id_map(&mapping, &target)?
            .map(|(s0, s1, id)| vec![s0.to_string(), s1.to_string(), id.to_string()])
            .collect();

        let expected = owned_string_vecs(vec![
            vec!["TACG", "AAAA", "BC0"],
            vec!["TAAG", "AAAA", "BC0"],
            vec!["TCAG", "AAAA", "BC0"],
            vec!["GACT", "TTTT", "BC1"],
            vec!["GCAT", "GGCC", "BC5"],
        ]);

        assert_eq!(translated, expected);

        // Translating the target whitelist should leave it unchanged.
        let translated_target: Vec<_> = target
            .create_translation_from_id_map(&mapping, &target)?
            .map(|(s0, s1, id)| vec![s0.to_string(), s1.to_string(), id.to_string()])
            .collect();

        assert_eq!(
            translated_target,
            owned_string_vecs(vec![
                vec!["ACTG", "AAAA", "BC0"],
                vec!["ATCG", "AAAA", "BC0"],
                vec!["CTAG", "TTTT", "BC1"],
            ])
        );

        Ok(())
    }

    fn create_translation_whitelist(
        input: Vec<Vec<&str>>,
    ) -> (WhitelistSource, tempfile::NamedTempFile) {
        let input_lines = owned_string_vecs(input);
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut writer = BufWriter::new(&mut file);
        for line in input_lines {
            writeln!(writer, "{}", line.join("\t")).unwrap();
        }
        writer.flush().unwrap();
        drop(writer);
        (
            WhitelistSource::DynamicTranslation {
                path: file.path().into(),
            },
            file,
        )
    }

    fn owned_string_vecs(input: Vec<Vec<&str>>) -> Vec<Vec<String>> {
        input
            .into_iter()
            .map(|inner| inner.into_iter().map(String::from).collect())
            .collect()
    }
}
