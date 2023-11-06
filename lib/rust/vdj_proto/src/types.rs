#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct AnnotationFeature {
    /// chain type of the reference record, e.g. TRA
    #[prost(enumeration = "VdjChain", tag = "1")]
    pub chain: i32,
    /// same as gene_name
    #[prost(string, tag = "2")]
    pub display_name: ::prost::alloc::string::String,
    /// id of reference record
    #[prost(uint32, tag = "3")]
    pub feature_id: u32,
    /// name of reference record e.g. TRAV14-1
    #[prost(string, tag = "4")]
    pub gene_name: ::prost::alloc::string::String,
    /// region type e.g. L-REGION+V-REGION
    #[prost(enumeration = "VdjRegion", tag = "5")]
    pub region_type: i32,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct AnnotationUnit {
    /// start on contig
    #[prost(uint32, tag = "1")]
    pub contig_match_start: u32,
    /// stop on contig
    #[prost(uint32, tag = "2")]
    pub contig_match_end: u32,
    /// start on reference record
    #[prost(uint32, tag = "3")]
    pub annotation_match_start: u32,
    /// stop on reference record
    #[prost(uint32, tag = "4")]
    pub annotation_match_end: u32,
    /// length of reference record
    #[prost(uint32, tag = "5")]
    pub annotation_length: u32,
    /// cigar of the alignment
    #[prost(string, tag = "6")]
    pub cigar: ::prost::alloc::string::String,
    /// score of the alignment
    #[prost(int32, tag = "7")]
    pub score: i32,
    /// feature type
    #[prost(message, optional, tag = "8")]
    pub feature: ::core::option::Option<AnnotationFeature>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GexData {
    #[prost(bool, tag = "1")]
    pub is_gex_cell: bool,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct AsmData {
    #[prost(bool, tag = "1")]
    pub is_asm_cell: bool,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct JunctionSupport {
    #[prost(int32, tag = "1")]
    pub reads: i32,
    #[prost(int32, tag = "2")]
    pub umis: i32,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Region {
    #[prost(int32, tag = "1")]
    pub start: i32,
    #[prost(int32, tag = "2")]
    pub stop: i32,
    #[prost(string, tag = "3")]
    pub nt_seq: ::prost::alloc::string::String,
    #[prost(string, tag = "4")]
    pub aa_seq: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ContigAnnotation {
    /// the barcode
    #[prost(string, tag = "1")]
    pub barcode: ::prost::alloc::string::String,
    /// name of the contig
    #[prost(string, tag = "2")]
    pub contig_name: ::prost::alloc::string::String,
    /// nucleotide sequence for contig
    #[prost(string, tag = "3")]
    pub sequence: ::prost::alloc::string::String,
    /// contig quality scores
    #[prost(string, tag = "4")]
    pub quals: ::prost::alloc::string::String,
    /// number of reads assigned to contig
    #[prost(uint64, tag = "5")]
    pub read_count: u64,
    /// number of UMIs assigned to the contig
    #[prost(uint64, tag = "6")]
    pub umi_count: u64,
    /// start pos on contig of start codon. -1 for None
    #[prost(int32, tag = "7")]
    pub start_codon_pos: i32,
    /// start pos on contig of stop codon. -1 for None
    #[prost(int32, tag = "8")]
    pub stop_codon_pos: i32,
    /// amino acid sequence. "" indicates None
    #[prost(string, tag = "9")]
    pub aa_sequence: ::prost::alloc::string::String,
    /// amino acid sequence for CDR3. "" indicates None
    #[prost(string, tag = "10")]
    pub cdr3: ::prost::alloc::string::String,
    /// nucleotide sequence for CDR3. "" indicates None
    #[prost(string, tag = "11")]
    pub cdr3_nt: ::prost::alloc::string::String,
    /// start position in bases on contig of CDR3. -1 for None
    #[prost(int32, tag = "12")]
    pub cdr3_start: i32,
    /// stop position in bases on contig of CDR3. -1 for None
    #[prost(int32, tag = "13")]
    pub cdr3_stop: i32,
    #[prost(message, repeated, tag = "14")]
    pub annotations: ::prost::alloc::vec::Vec<AnnotationUnit>,
    /// "" indicates None
    #[prost(string, tag = "15")]
    pub clonotype: ::prost::alloc::string::String,
    /// "" indicates None
    #[prost(string, tag = "16")]
    pub raw_clonotype_id: ::prost::alloc::string::String,
    /// "" indicates None
    #[prost(string, tag = "17")]
    pub raw_consensus_id: ::prost::alloc::string::String,
    #[prost(bool, tag = "18")]
    pub high_confidence: bool,
    #[prost(bool, tag = "19")]
    pub is_cell: bool,
    #[prost(bool, tag = "20")]
    pub productive: bool,
    #[prost(bool, tag = "21")]
    pub filtered: bool,
    /// -1 for None
    #[prost(int32, tag = "22")]
    pub frame: i32,
    #[prost(message, optional, tag = "23")]
    pub asm_data: ::core::option::Option<AsmData>,
    #[prost(message, optional, tag = "24")]
    pub gex_data: ::core::option::Option<GexData>,
    #[prost(bool, tag = "25")]
    pub full_length: bool,
    #[prost(message, optional, tag = "26")]
    pub fwr1: ::core::option::Option<Region>,
    #[prost(message, optional, tag = "27")]
    pub cdr1: ::core::option::Option<Region>,
    #[prost(message, optional, tag = "28")]
    pub fwr2: ::core::option::Option<Region>,
    #[prost(message, optional, tag = "29")]
    pub cdr2: ::core::option::Option<Region>,
    #[prost(message, optional, tag = "30")]
    pub fwr3: ::core::option::Option<Region>,
    #[prost(message, optional, tag = "31")]
    pub fwr4: ::core::option::Option<Region>,
    /// "" indicates None
    #[prost(string, tag = "32")]
    pub exact_subclonotype_id: ::prost::alloc::string::String,
    #[prost(message, optional, tag = "33")]
    pub junction_support: ::core::option::Option<JunctionSupport>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct VdjReferenceRaw {
    /// regions.fa as a string
    #[prost(string, tag = "1")]
    pub regions: ::prost::alloc::string::String,
    /// reference.json as a string
    #[prost(string, tag = "2")]
    pub ref_json: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct VdjMetadata {
    #[prost(string, tag = "1")]
    pub reference_fasta_hash: ::prost::alloc::string::String,
    #[prost(string, tag = "2")]
    pub pipeline_version: ::prost::alloc::string::String,
    #[prost(enumeration = "Receptor", tag = "3")]
    pub receptor: i32,
    #[prost(uint32, repeated, tag = "4")]
    pub gem_wells: ::prost::alloc::vec::Vec<u32>,
    #[prost(uint32, tag = "5")]
    pub number_of_cells: u32,
    #[prost(string, tag = "6")]
    pub sample_id: ::prost::alloc::string::String,
    #[prost(string, tag = "7")]
    pub sample_desc: ::prost::alloc::string::String,
    #[prost(string, tag = "8")]
    pub multi_config_sha: ::prost::alloc::string::String,
    #[prost(string, tag = "9")]
    pub protobuf_version: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MetricsSummary {
    #[prost(string, tag = "1")]
    pub raw_json: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BarcodeData {
    #[prost(string, tag = "1")]
    pub barcode: ::prost::alloc::string::String,
    /// number of reads assigned to barcode
    #[prost(uint64, tag = "2")]
    pub read_count: u64,
    /// number of UMIs assigned to barcode
    #[prost(uint64, tag = "3")]
    pub umi_count: u64,
    /// num reads for each surviving nonsolo UMI
    #[prost(int32, repeated, tag = "4")]
    pub xucounts: ::prost::alloc::vec::Vec<i32>,
    /// number of contigs assigned to barcode
    #[prost(uint64, tag = "5")]
    pub ncontigs: u64,
    /// frac reads used in assembly
    #[prost(double, tag = "6")]
    pub frac_reads_used: f64,
}
/// This is the message written out in the proto file
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct VdjProtoMessage {
    #[prost(oneof = "vdj_proto_message::MessageContent", tags = "1, 2, 3, 4, 5")]
    pub message_content: ::core::option::Option<vdj_proto_message::MessageContent>,
}
/// Nested message and enum types in `VdjProtoMessage`.
pub mod vdj_proto_message {
    #[allow(clippy::large_enum_variant)]
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum MessageContent {
        #[prost(message, tag = "1")]
        Metadata(super::VdjMetadata),
        #[prost(message, tag = "2")]
        Reference(super::VdjReferenceRaw),
        #[prost(message, tag = "3")]
        Annotation(super::ContigAnnotation),
        #[prost(message, tag = "4")]
        Metrics(super::MetricsSummary),
        #[prost(message, tag = "5")]
        BarcodeData(super::BarcodeData),
    }
}
/// Various regions within a VDJ transcript
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum VdjRegion {
    /// 5' untranslated region
    U = 0,
    /// Variable region
    V = 1,
    /// Diversity region
    D = 2,
    /// Joining region
    J = 3,
    /// Constant region
    C = 4,
}
impl VdjRegion {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            VdjRegion::U => "U",
            VdjRegion::V => "V",
            VdjRegion::D => "D",
            VdjRegion::J => "J",
            VdjRegion::C => "C",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "U" => Some(Self::U),
            "V" => Some(Self::V),
            "D" => Some(Self::D),
            "J" => Some(Self::J),
            "C" => Some(Self::C),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum VdjChain {
    Igh = 0,
    Igk = 1,
    Igl = 2,
    Tra = 3,
    Trb = 4,
    Trd = 5,
    Trg = 6,
}
impl VdjChain {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            VdjChain::Igh => "IGH",
            VdjChain::Igk => "IGK",
            VdjChain::Igl => "IGL",
            VdjChain::Tra => "TRA",
            VdjChain::Trb => "TRB",
            VdjChain::Trd => "TRD",
            VdjChain::Trg => "TRG",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "IGH" => Some(Self::Igh),
            "IGK" => Some(Self::Igk),
            "IGL" => Some(Self::Igl),
            "TRA" => Some(Self::Tra),
            "TRB" => Some(Self::Trb),
            "TRD" => Some(Self::Trd),
            "TRG" => Some(Self::Trg),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Receptor {
    Tcr = 0,
    Ig = 1,
    Tcrgd = 2,
}
impl Receptor {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            Receptor::Tcr => "TCR",
            Receptor::Ig => "IG",
            Receptor::Tcrgd => "TCRGD",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "TCR" => Some(Self::Tcr),
            "IG" => Some(Self::Ig),
            "TCRGD" => Some(Self::Tcrgd),
            _ => None,
        }
    }
}
