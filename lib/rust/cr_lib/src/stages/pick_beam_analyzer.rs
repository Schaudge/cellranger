//! Martian stage PICK_BEAM_ANALYZER

use super::compute_antigen_vdj_metrics::AntigenVdjMetricsFormat;
use crate::utils::hard_link_martianfile;
use anyhow::Result;
use martian::{MartianMain, MartianRover};
use martian_derive::{make_mro, MartianStruct};
use martian_filetypes::json_file::JsonFile;
use martian_filetypes::tabular_file::CsvFile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, MartianStruct)]
pub struct BeamAnalyzerOutputs {
    pub antigen_specificity_scores: Option<CsvFile<()>>,
    pub antigen_assignment: Option<CsvFile<()>>,
    pub clonotype_concordance: Option<CsvFile<()>>,
    pub exact_subclonotype_concordance: Option<CsvFile<()>>,
    pub specificity_summary: Option<JsonFile<()>>,
    pub antigen_vdj_metrics_json: Option<JsonFile<()>>,
    pub antigen_vdj_metrics_bin: Option<AntigenVdjMetricsFormat>,
    pub per_barcode: Option<CsvFile<()>>,
}

impl BeamAnalyzerOutputs {
    fn hard_link(self, rover: &MartianRover) -> Result<Self> {
        let BeamAnalyzerOutputs {
            antigen_specificity_scores,
            antigen_assignment,
            clonotype_concordance,
            exact_subclonotype_concordance,
            specificity_summary,
            antigen_vdj_metrics_json,
            antigen_vdj_metrics_bin,
            per_barcode,
        } = self;
        Ok(BeamAnalyzerOutputs {
            antigen_specificity_scores: antigen_specificity_scores
                .map(|x| hard_link_martianfile(x, rover))
                .transpose()?,
            antigen_assignment: antigen_assignment
                .map(|x| hard_link_martianfile(x, rover))
                .transpose()?,
            clonotype_concordance: clonotype_concordance
                .map(|x| hard_link_martianfile(x, rover))
                .transpose()?,
            exact_subclonotype_concordance: exact_subclonotype_concordance
                .map(|x| hard_link_martianfile(x, rover))
                .transpose()?,
            specificity_summary: specificity_summary
                .map(|x| hard_link_martianfile(x, rover))
                .transpose()?,
            antigen_vdj_metrics_json: antigen_vdj_metrics_json
                .map(|x| hard_link_martianfile(x, rover))
                .transpose()?,
            antigen_vdj_metrics_bin: antigen_vdj_metrics_bin
                .map(|x| hard_link_martianfile(x, rover))
                .transpose()?,
            per_barcode: per_barcode
                .map(|x| hard_link_martianfile(x, rover))
                .transpose()?,
        })
    }
}

/// The Martian stage inputs.
#[derive(Clone, Deserialize, MartianStruct)]
pub struct PickBeamAnalyzerStageInputs {
    pub options: Vec<Option<BeamAnalyzerOutputs>>,
}

/// The Martian stage outputs.
#[derive(Clone, Serialize, Deserialize, MartianStruct)]
pub struct PickBeamAnalyzerStageOutputs {
    pub output: Option<BeamAnalyzerOutputs>,
}

pub struct PickBeamAnalyzer;

#[make_mro(volatile = strict)]
impl MartianMain for PickBeamAnalyzer {
    type StageInputs = PickBeamAnalyzerStageInputs;
    type StageOutputs = PickBeamAnalyzerStageOutputs;

    fn main(&self, args: Self::StageInputs, rover: MartianRover) -> Result<Self::StageOutputs> {
        Ok(Self::StageOutputs {
            output: {
                let mut options: Vec<_> = args.options.into_iter().flatten().collect();
                assert!(options.len() <= 1);
                options
                    .pop()
                    .map(|outs| outs.hard_link(&rover))
                    .transpose()?
            },
        })
    }
}
