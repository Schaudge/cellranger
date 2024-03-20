# Copyright (c) 2019 10X Genomics, Inc. All rights reserved.
"""Iterate over the input molecule info file and calculate on-target and off-target on a per-read basis.

These metrics are stratified into a data loss hierarchy, where at each step of data loss
the number of on-target vs. off-target are quantified.

Also adds useful information for targeting performance, such as the classification of genes
based on Reads per UMI.
"""

import json
import math
import os

import numpy as np

import cellranger.matrix as cr_matrix
import cellranger.pandas_utils as pdu
import tenkit.safe_json as tk_safe_json
from cellranger.molecule_counter import MoleculeCounter
from cellranger.pandas_utils import FEATURE_DF_UMI_COL
from cellranger.rna.library import GENE_EXPRESSION_LIBRARY_TYPE
from cellranger.targeted.gmm import fit_enrichments
from cellranger.targeted.targeted_spatial import SPATIAL_TARGET_DISALLOWED_PANEL_TYPES
from cellranger.targeted.utils import BOTH_SPHERICAL, BOTH_TIED, OFFTARGETS_ONLY
from cellranger.webshim.constants.gex import (
    ENRICHMENT_COLNAME,
    LOG_RPU_CELLS_COLNAME,
    TARGETING_COLNAME,
)
from tenkit.stats import robust_divide

__MRO__ = """
stage CALCULATE_TARGETED_METRICS(
    in  h5       molecule_info,
    in  h5       filtered_gene_bc_matrices,
    in  json     basic_counter_summary,
    in  tps.json target_panel_summary,
    in  bool     is_spatial,
    out json     summary,
    out csv      per_feature_metrics_csv,
    src py       "stages/targeted/calculate_targeted_metrics",
) split (
)
"""

MIN_UMIS = 10
MIN_RPU_THRESHOLD = 2.5
TARGETED_RPU_METRIC_KEY = "mean_reads_per_umi_per_gene_cells_on_target"


def split(args):
    fbm_bytes = os.stat(args.filtered_gene_bc_matrices).st_size
    mem_gib = 3 + math.ceil(11.7 * fbm_bytes / 1024**3)
    print(f"{fbm_bytes=},{mem_gib=}")
    return {"chunks": [], "join": {"__mem_gb": mem_gib}}


# pylint: disable=invalid-name, singleton-comparison
def get_enrichment_metrics(
    is_spatial,
    pfm,
    disable_rpu_enrichments,
    method=OFFTARGETS_ONLY,
    THRESHOLD_FRAC=0.5,
):
    """Computes a log_rpu_threshold above which to consider a gene enriched, and.

    classifies all genes as enriched on non-enriched according to this.
    """
    tgt_label = pfm[pfm[f"{FEATURE_DF_UMI_COL}_cells"] >= MIN_UMIS][TARGETING_COLNAME].to_numpy()
    values = pfm[pfm[f"{FEATURE_DF_UMI_COL}_cells"] >= MIN_UMIS][LOG_RPU_CELLS_COLNAME].to_numpy()

    enrichment_params, class_stats = fit_enrichments(tgt_label, values, method=method)

    log_rpu_threshold = enrichment_params.log_rpu_threshold
    enrichment_calc_metrics = {
        "log_rpu_threshold": enrichment_params.log_rpu_threshold,
        "lrpu_fitted_mean_1": enrichment_params.mu_high,
        "lrpu_fitted_mean_2": enrichment_params.mu_low,
        "lrpu_fitted_sd_1": enrichment_params.sd_high,
        "lrpu_fitted_sd_2": enrichment_params.sd_low,
        "lrpu_fitted_weight_1": enrichment_params.alpha_high,
        "lrpu_fitted_weight_2": enrichment_params.alpha_low,
        "frac_on_target_genes_enriched": robust_divide(
            class_stats.n_targeted_enriched,
            class_stats.n_targeted_enriched + class_stats.n_targeted_not_enriched,
        ),
        "frac_off_target_genes_enriched": np.nanmin(
            [
                robust_divide(
                    class_stats.n_offtgt_enriched,
                    class_stats.n_offtgt_enriched + class_stats.n_targeted_enriched,
                ),
                robust_divide(
                    class_stats.n_offtgt_enriched,
                    class_stats.n_offtgt_enriched + class_stats.n_offtgt_not_enriched,
                ),
            ]
        ),
    }
    # These metrics are duplicated to provide flexibility between the two systems. One that uses a CSV + JSON
    # to generate the outs/summary_metrics.csv and the websummary and one that exclusively uses the CSV.
    if is_spatial:
        enrichment_calc_metrics.update(
            {
                "spatial_num_rpu_enriched_genes_on_target": class_stats.n_targeted_enriched,
                "spatial_num_rpu_non_enriched_genes_on_target": class_stats.n_targeted_not_enriched,
                "spatial_num_rpu_enriched_genes_off_target": class_stats.n_offtgt_enriched,
                "spatial_num_rpu_non_enriched_genes_off_target": class_stats.n_offtgt_not_enriched,
            }
        )

    enrichment_calc_metrics.update(
        {
            "num_rpu_enriched_genes_on_target": class_stats.n_targeted_enriched,
            "num_rpu_non_enriched_genes_on_target": class_stats.n_targeted_not_enriched,
            "num_rpu_enriched_genes_off_target": class_stats.n_offtgt_enriched,
            "num_rpu_non_enriched_genes_off_target": class_stats.n_offtgt_not_enriched,
        }
    )

    # if we think enrichments look bad and it's because depth is too low, disable enrichment reporting
    if not np.isnan(log_rpu_threshold) and disable_rpu_enrichments:
        frac_on_target_genes_enriched = enrichment_calc_metrics["frac_on_target_genes_enriched"]
        frac_off_target_genes_enriched = enrichment_calc_metrics["frac_off_target_genes_enriched"]
        if (
            np.isnan(frac_on_target_genes_enriched)
            or frac_on_target_genes_enriched < THRESHOLD_FRAC
            or (
                method in [BOTH_TIED, BOTH_SPHERICAL]
                and not np.isnan(frac_off_target_genes_enriched)
                and frac_off_target_genes_enriched > THRESHOLD_FRAC
            )
        ):
            enrichment_calc_metrics = {
                "log_rpu_threshold": np.nan,
                "lrpu_fitted_mean_1": np.nan,
                "lrpu_fitted_mean_2": np.nan,
                "lrpu_fitted_sd_1": np.nan,
                "lrpu_fitted_sd_2": np.nan,
                "lrpu_fitted_weight_1": np.nan,
                "lrpu_fitted_weight_2": np.nan,
                "spatial_num_rpu_enriched_genes_on_target": np.nan,
                "spatial_num_rpu_non_enriched_genes_on_target": np.nan,
                "spatial_num_rpu_enriched_genes_off_target": np.nan,
                "spatial_num_rpu_non_enriched_genes_off_target": np.nan,
                "num_rpu_enriched_genes_on_target": np.nan,
                "num_rpu_non_enriched_genes_on_target": np.nan,
                "num_rpu_enriched_genes_off_target": np.nan,
                "num_rpu_non_enriched_genes_off_target": np.nan,
                "frac_on_target_genes_enriched": np.nan,
                "frac_off_target_genes_enriched": np.nan,
            }
            log_rpu_threshold = np.nan

    if np.isnan(log_rpu_threshold):
        pfm[ENRICHMENT_COLNAME] = np.nan
    else:
        pfm[ENRICHMENT_COLNAME] = pfm[LOG_RPU_CELLS_COLNAME] > log_rpu_threshold
        if method == OFFTARGETS_ONLY:
            pfm.loc[pfm[TARGETING_COLNAME] == False][ENRICHMENT_COLNAME] = np.nan

    # nomenclature is confusing, but both quantities are wanted:
    # detected -> >= 1 cell-UMI count (named for consistency with GEX)
    # quantifiable -> >= MIN_UMIS (10) cell-UMI counts (can be used in estimating enrichments)
    n_on_target_genes_not_detected = (
        (pfm[TARGETING_COLNAME] == 1) & (pfm[f"{FEATURE_DF_UMI_COL}_cells"] == 0)
    ).sum()
    n_off_target_genes_not_detected = (
        (pfm[TARGETING_COLNAME] == 0) & (pfm[f"{FEATURE_DF_UMI_COL}_cells"] == 0)
    ).sum()
    n_on_target_genes_not_quantifiable = (
        (pfm[TARGETING_COLNAME] == 1) & (pfm[f"{FEATURE_DF_UMI_COL}_cells"] < MIN_UMIS)
    ).sum()
    n_off_target_genes_not_quantifiable = (
        (pfm[TARGETING_COLNAME] == 0) & (pfm[f"{FEATURE_DF_UMI_COL}_cells"] < MIN_UMIS)
    ).sum()

    n_on_target_genes = pfm[pfm[TARGETING_COLNAME] == True].shape[0]
    n_off_target_genes = pfm[pfm[TARGETING_COLNAME] == False].shape[0]

    enrichment_calc_metrics.update(
        {
            "num_genes_not_detected_on_target": n_on_target_genes_not_detected,
            "num_genes_not_detected_off_target": n_off_target_genes_not_detected,
            "num_genes_detected_on_target": n_on_target_genes - n_on_target_genes_not_detected,
            "num_genes_detected_off_target": n_off_target_genes - n_off_target_genes_not_detected,
            "num_genes_not_quantifiable_on_target": n_on_target_genes_not_quantifiable,
            "num_genes_not_quantifiable_off_target": n_off_target_genes_not_quantifiable,
        }
    )

    if is_spatial:
        enrichment_calc_metrics.update(
            {
                "spatial_num_genes_on_target": n_on_target_genes,
                "spatial_num_genes_off_target": n_off_target_genes,
                "spatial_num_genes_quantifiable_on_target": n_on_target_genes
                - n_on_target_genes_not_quantifiable,
                "spatial_num_genes_quantifiable_off_target": n_off_target_genes
                - n_off_target_genes_not_quantifiable,
            }
        )

    enrichment_calc_metrics.update(
        {
            "num_genes_on_target": n_on_target_genes,
            "num_genes_off_target": n_off_target_genes,
            "num_genes_quantifiable_on_target": n_on_target_genes
            - n_on_target_genes_not_quantifiable,
            "num_genes_quantifiable_off_target": n_off_target_genes
            - n_off_target_genes_not_quantifiable,
        }
    )

    return pfm, enrichment_calc_metrics


def get_mean_per_gene_rpu_metrics(is_spatial, pfm):
    """Compute reads per UMI statistics per gene, for targeted and off-target genes."""
    per_gene_rpu_metrics = {}

    for targeting in [True, False]:
        target_suffix = "on_target" if targeting else "off_target"
        for in_cells in [True, False]:
            in_cells_suffix = "_cells" if in_cells else ""
            reads_per_umi = pfm[
                (pfm[TARGETING_COLNAME] == targeting) & (pfm["num_umis_cells"] >= MIN_UMIS)
            ][f"mean_reads_per_umi{in_cells_suffix}"]
            if reads_per_umi.shape[0] == 0:
                mean_rpu = 0.0
                cv_rpu = 0.0
                median_rpu = 0.0
                iqrnorm_rpu = 0.0
                perc80_rpu = 0.0
            else:
                mean_rpu = reads_per_umi.mean(skipna=True)
                std_rpu = reads_per_umi.std(skipna=True)
                cv_rpu = robust_divide(std_rpu, mean_rpu)
                median_rpu = reads_per_umi.median(skipna=True)
                iqrnorm_rpu = robust_divide(
                    reads_per_umi.quantile(0.75) - reads_per_umi.quantile(0.25), median_rpu
                )
                perc80_rpu = reads_per_umi.quantile(0.80)
            if is_spatial:
                per_gene_rpu_metrics[
                    f"spatial_mean_reads_per_umi_per_gene{in_cells_suffix}_{target_suffix}"
                ] = mean_rpu

            per_gene_rpu_metrics[
                f"mean_reads_per_umi_per_gene{in_cells_suffix}_{target_suffix}"
            ] = mean_rpu
            per_gene_rpu_metrics[
                f"cv_reads_per_umi_per_gene{in_cells_suffix}_{target_suffix}"
            ] = cv_rpu
            per_gene_rpu_metrics[
                f"median_reads_per_umi_per_gene{in_cells_suffix}_{target_suffix}"
            ] = median_rpu
            per_gene_rpu_metrics[
                f"iqrnorm_reads_per_umi_per_gene{in_cells_suffix}_{target_suffix}"
            ] = iqrnorm_rpu
            per_gene_rpu_metrics[
                f"perc80_reads_per_umi_per_gene{in_cells_suffix}_{target_suffix}"
            ] = perc80_rpu

    return per_gene_rpu_metrics


def get_feature_summary_df(molecule_info_fn):
    """Returns a dataframe with UMI and read counts per gene."""
    with MoleculeCounter.open(molecule_info_fn, "r") as mc:
        feature_summary_df = pdu.collapse_feature_counts(
            mc,
            filter_library_idx=mc.get_library_indices_by_type()[GENE_EXPRESSION_LIBRARY_TYPE],
        )
        feature_ref = mc.feature_reference
        target_gene_indices = feature_ref.get_target_feature_indices()
        target_gene_ids = [feature_ref.feature_defs[i].id for i in target_gene_indices]

    # filter to GEX features
    feature_summary_df = feature_summary_df[
        feature_summary_df["feature_type"] == GENE_EXPRESSION_LIBRARY_TYPE
    ]

    # add targeting colname
    feature_summary_df[TARGETING_COLNAME] = feature_summary_df["feature_id"].isin(target_gene_ids)

    feature_summary_df["mean_reads_per_umi"] = feature_summary_df["num_reads"].divide(
        other=feature_summary_df["num_umis"]
    )
    feature_summary_df["mean_reads_per_umi_log10"] = np.log10(
        feature_summary_df["mean_reads_per_umi"]
    )
    feature_summary_df["mean_reads_per_umi_cells"] = feature_summary_df["num_reads_cells"].divide(
        other=feature_summary_df["num_umis_cells"]
    )
    feature_summary_df["mean_reads_per_umi_cells_log10"] = np.log10(
        feature_summary_df["mean_reads_per_umi_cells"]
    )
    return feature_summary_df


# pylint: disable=too-many-locals
def join(args, outs, chunk_defs, chunk_outs):
    # need to translate and use a couple metrics from the original summary
    with open(args.basic_counter_summary) as in_f:
        basic_counter_metrics = json.load(in_f)
    targeted_metrics = {}

    # add a few useful metrics from count matrix to targeted metrics
    matrix = cr_matrix.CountMatrix.load_h5_file(args.filtered_gene_bc_matrices)
    target_feature_indices = matrix.feature_ref.get_target_feature_indices()
    matrix_view = matrix.view().select_features(target_feature_indices)

    median_on_target_umis_per_cell = np.nanmedian(matrix_view.sum(axis=0))
    median_on_target_genes_per_cell = np.nanmedian(matrix_view.count_ge(axis=0, threshold=1))
    _, num_cells, _ = cr_matrix.CountMatrix.load_dims_from_h5(args.filtered_gene_bc_matrices)

    with MoleculeCounter.open(args.molecule_info, "r") as mc:
        gex_library_indices = mc.get_library_indices_by_type()[GENE_EXPRESSION_LIBRARY_TYPE]
        raw_reads_per_lib = mc.get_raw_read_pairs_per_library()
        total_reads = np.sum([raw_reads_per_lib[idx] for idx in gex_library_indices])
    # This can be a NaN so need to convert before multiplying below
    multi_transcriptome_targeted_conf_mapped_reads_frac_float = float(
        basic_counter_metrics["multi_transcriptome_targeted_conf_mapped_reads_frac"]
    )
    mean_targeted_reads_per_cell = robust_divide(
        multi_transcriptome_targeted_conf_mapped_reads_frac_float * total_reads,
        num_cells,
    )
    targeted_metrics.update(
        {
            "median_umis_per_cell_on_target": median_on_target_umis_per_cell,
            "median_genes_per_cell_on_target": median_on_target_genes_per_cell,
            "total_targeted_reads_per_filtered_bc": mean_targeted_reads_per_cell,
            # rewrite these metrics with usual suffix notation for easier use in WS
            "multi_frac_conf_transcriptomic_reads_on_target": basic_counter_metrics[
                "multi_transcriptome_targeted_conf_mapped_reads_frac"
            ],
            "multi_frac_conf_transcriptomic_reads_off_target": basic_counter_metrics[
                "multi_transcriptome_untargeted_conf_mapped_reads_frac"
            ],
        }
    )

    # add a warning flag if we're processing an unsupported targeted panel for spatial
    if args.is_spatial:
        with open(args.target_panel_summary) as in_f:
            target_panel_summary = json.load(in_f)
        targeted_metrics["targeted_unsupported_panel"] = (
            target_panel_summary["target_panel_type"] in SPATIAL_TARGET_DISALLOWED_PANEL_TYPES
        )

    # get table of per_feature_metrics
    feature_summary_df = get_feature_summary_df(args.molecule_info)

    # add targeted sequencing saturation
    total_targeted_reads = feature_summary_df[feature_summary_df[TARGETING_COLNAME]][
        "num_reads"
    ].sum()
    total_targeted_umis = feature_summary_df[feature_summary_df[TARGETING_COLNAME]][
        "num_umis"
    ].sum()
    targeted_sequencing_saturation = robust_divide(
        total_targeted_reads - total_targeted_umis, total_targeted_reads
    )
    targeted_metrics["multi_cdna_pcr_dupe_reads_frac_on_target"] = targeted_sequencing_saturation

    # get RPU metrics
    targeted_metrics.update(get_mean_per_gene_rpu_metrics(args.is_spatial, feature_summary_df))
    disable_rpu_enrichments = np.isnan(targeted_metrics[TARGETED_RPU_METRIC_KEY]) or (
        targeted_metrics[TARGETED_RPU_METRIC_KEY] < MIN_RPU_THRESHOLD
    )

    # get per gene enrichments
    feature_summary_df, enrichment_calc_metrics = get_enrichment_metrics(
        args.is_spatial, feature_summary_df, disable_rpu_enrichments, method=BOTH_TIED
    )
    targeted_metrics.update(enrichment_calc_metrics)

    feature_summary_df.to_csv(outs.per_feature_metrics_csv, index=False)

    with open(outs.summary, "w") as outf:
        tk_safe_json.dump_numpy(targeted_metrics, outf)
