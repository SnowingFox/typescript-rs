//! `BuildInfo`: the serialized `.tsbuildinfo` payload.
//!
//! 1:1 port of the reachable subset of Go
//! `internal/execute/incremental/buildInfo.go`. File ids are 1-based indices
//! into [`BuildInfo::file_names`]; file-id-list ids are 1-based indices into
//! [`BuildInfo::file_ids_list`]. The compact JSON encoding (bare-string file
//! infos, `[start, end]` roots, `[fileId, fileIdListId]` referenced-map entries,
//! bare-`fileId` semantic diagnostics) is matched byte-for-byte against
//! `cmd/tsgo`.

use indexmap::IndexMap;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::snapshot::{FileInfo, RESOLUTION_MODE_COMMON_JS};

/// A 1-based index into [`BuildInfo::file_names`] (`0` means "none").
// Go: internal/execute/incremental/buildInfo.go:BuildInfoFileId
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct BuildInfoFileId(pub u32);

/// A 1-based index into [`BuildInfo::file_ids_list`] (`0` means "none").
// Go: internal/execute/incremental/buildInfo.go:BuildInfoFileIdListId
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct BuildInfoFileIdListId(pub u32);

/// A root-file marker: a consecutive id range `[start, end]`, a single id, or a
/// non-incremental root file name.
// Go: internal/execute/incremental/buildInfo.go:BuildInfoRoot
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildInfoRoot {
    pub start: BuildInfoFileId,
    pub end: BuildInfoFileId,
    pub non_incremental: String,
}

impl BuildInfoRoot {
    /// A consecutive `[start, end]` root id range.
    pub fn range(start: BuildInfoFileId, end: BuildInfoFileId) -> Self {
        Self {
            start,
            end,
            non_incremental: String::new(),
        }
    }

    /// A single root file id.
    pub fn single(start: BuildInfoFileId) -> Self {
        Self {
            start,
            end: BuildInfoFileId(0),
            non_incremental: String::new(),
        }
    }

    /// A non-incremental root file name.
    pub fn non_incremental(name: impl Into<String>) -> Self {
        Self {
            start: BuildInfoFileId(0),
            end: BuildInfoFileId(0),
            non_incremental: name.into(),
        }
    }
}

// Go: internal/execute/incremental/buildInfo.go:BuildInfoRoot.MarshalJSON
impl Serialize for BuildInfoRoot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.start.0 != 0 {
            if self.end.0 != 0 {
                [self.start.0, self.end.0].serialize(serializer)
            } else {
                self.start.0.serialize(serializer)
            }
        } else {
            self.non_incremental.serialize(serializer)
        }
    }
}

// Go: internal/execute/incremental/buildInfo.go:BuildInfoRoot.UnmarshalJSON
impl<'de> Deserialize<'de> for BuildInfoRoot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match serde_json::Value::deserialize(deserializer)? {
            serde_json::Value::Array(a) if a.len() == 2 => Ok(BuildInfoRoot::range(
                BuildInfoFileId(json_u32::<D>(&a[0])?),
                BuildInfoFileId(json_u32::<D>(&a[1])?),
            )),
            serde_json::Value::Number(n) => Ok(BuildInfoRoot::single(BuildInfoFileId(
                n.as_u64()
                    .ok_or_else(|| D::Error::custom("invalid BuildInfoRoot id"))?
                    as u32,
            ))),
            serde_json::Value::String(s) => Ok(BuildInfoRoot::non_incremental(s)),
            other => Err(D::Error::custom(format!("invalid BuildInfoRoot: {other}"))),
        }
    }
}

/// Compact file-info object for the no-signature form.
// Go: internal/execute/incremental/buildInfo.go:buildInfoFileInfoNoSignature
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfoFileInfoNoSignature {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_signature: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub affects_global_scope: bool,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub implied_node_format: i32,
}

/// Compact file-info object for the with-signature form.
// Go: internal/execute/incremental/buildInfo.go:buildInfoFileInfoWithSignature
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfoFileInfoWithSignature {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signature: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub affects_global_scope: bool,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub implied_node_format: i32,
}

/// A file's serialized info: a bare signature string, a no-signature object, or
/// a with-signature object (mirroring Go's union).
// Go: internal/execute/incremental/buildInfo.go:BuildInfoFileInfo
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildInfoFileInfo {
    signature: Option<String>,
    no_signature: Option<BuildInfoFileInfoNoSignature>,
    file_info: Option<BuildInfoFileInfoWithSignature>,
}

impl BuildInfoFileInfo {
    /// The bare-string (signature-only) form.
    pub fn signature(signature: impl Into<String>) -> Self {
        Self {
            signature: Some(signature.into()),
            no_signature: None,
            file_info: None,
        }
    }

    /// Builds the compact form from a [`FileInfo`], mirroring Go
    /// `newBuildInfoFileInfo`.
    // Go: internal/execute/incremental/buildInfo.go:newBuildInfoFileInfo
    pub fn from_file_info(info: &FileInfo) -> Self {
        if info.version == info.signature {
            if !info.affects_global_scope && info.implied_node_format == RESOLUTION_MODE_COMMON_JS {
                return Self::signature(info.signature.clone());
            }
        } else if info.signature.is_empty() {
            return Self {
                signature: None,
                no_signature: Some(BuildInfoFileInfoNoSignature {
                    version: info.version.clone(),
                    no_signature: true,
                    affects_global_scope: info.affects_global_scope,
                    implied_node_format: info.implied_node_format,
                }),
                file_info: None,
            };
        }
        Self {
            signature: None,
            no_signature: None,
            file_info: Some(BuildInfoFileInfoWithSignature {
                version: info.version.clone(),
                signature: if info.signature == info.version {
                    String::new()
                } else {
                    info.signature.clone()
                },
                affects_global_scope: info.affects_global_scope,
                implied_node_format: info.implied_node_format,
            }),
        }
    }

    /// Decodes back to a [`FileInfo`], mirroring Go `GetFileInfo`.
    // Go: internal/execute/incremental/buildInfo.go:BuildInfoFileInfo.GetFileInfo
    pub fn get_file_info(&self) -> FileInfo {
        if let Some(sig) = &self.signature {
            return FileInfo {
                version: sig.clone(),
                signature: sig.clone(),
                affects_global_scope: false,
                implied_node_format: RESOLUTION_MODE_COMMON_JS,
            };
        }
        if let Some(ns) = &self.no_signature {
            return FileInfo {
                version: ns.version.clone(),
                signature: String::new(),
                affects_global_scope: ns.affects_global_scope,
                implied_node_format: ns.implied_node_format,
            };
        }
        let fi = self.file_info.clone().unwrap_or_default();
        FileInfo {
            version: fi.version.clone(),
            signature: if fi.signature.is_empty() {
                fi.version
            } else {
                fi.signature
            },
            affects_global_scope: fi.affects_global_scope,
            implied_node_format: fi.implied_node_format,
        }
    }

    /// Whether this entry is the bare-string (signature-only) form.
    // Go: internal/execute/incremental/buildInfo.go:BuildInfoFileInfo.HasSignature
    pub fn has_signature(&self) -> bool {
        self.signature.is_some()
    }
}

// Go: internal/execute/incremental/buildInfo.go:BuildInfoFileInfo.MarshalJSON
impl Serialize for BuildInfoFileInfo {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(sig) = &self.signature {
            sig.serialize(serializer)
        } else if let Some(ns) = &self.no_signature {
            ns.serialize(serializer)
        } else if let Some(fi) = &self.file_info {
            fi.serialize(serializer)
        } else {
            BuildInfoFileInfoWithSignature::default().serialize(serializer)
        }
    }
}

// Go: internal/execute/incremental/buildInfo.go:BuildInfoFileInfo.UnmarshalJSON
impl<'de> Deserialize<'de> for BuildInfoFileInfo {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) => Ok(BuildInfoFileInfo::signature(s)),
            serde_json::Value::Object(ref map) => {
                if map.get("noSignature").and_then(serde_json::Value::as_bool) == Some(true) {
                    let ns: BuildInfoFileInfoNoSignature =
                        serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Self {
                        signature: None,
                        no_signature: Some(ns),
                        file_info: None,
                    })
                } else {
                    let fi: BuildInfoFileInfoWithSignature =
                        serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Self {
                        signature: None,
                        no_signature: None,
                        file_info: Some(fi),
                    })
                }
            }
            other => Err(D::Error::custom(format!(
                "invalid BuildInfoFileInfo: {other}"
            ))),
        }
    }
}

/// A `referencedMap` entry: `[fileId, fileIdListId]`.
// Go: internal/execute/incremental/buildInfo.go:BuildInfoReferenceMapEntry
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildInfoReferenceMapEntry {
    pub file_id: BuildInfoFileId,
    pub file_id_list_id: BuildInfoFileIdListId,
}

// Go: internal/execute/incremental/buildInfo.go:BuildInfoReferenceMapEntry.MarshalJSON
impl Serialize for BuildInfoReferenceMapEntry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        [self.file_id.0, self.file_id_list_id.0].serialize(serializer)
    }
}

// Go: internal/execute/incremental/buildInfo.go:BuildInfoReferenceMapEntry.UnmarshalJSON
impl<'de> Deserialize<'de> for BuildInfoReferenceMapEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let [file_id, file_id_list_id] = <[u32; 2]>::deserialize(deserializer)?;
        Ok(Self {
            file_id: BuildInfoFileId(file_id),
            file_id_list_id: BuildInfoFileIdListId(file_id_list_id),
        })
    }
}

/// A `semanticDiagnosticsPerFile` entry. In the reachable subset this is a bare
/// `fileId` (a file with no cached diagnostics); the cached-diagnostics object
/// form is preserved verbatim for round-tripping (full model is DEFER).
// Go: internal/execute/incremental/buildInfo.go:BuildInfoSemanticDiagnostic
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BuildInfoSemanticDiagnostic {
    pub file_id: BuildInfoFileId,
    // DEFER(P6-9b): full `BuildInfoDiagnosticsOfFile` model. blocked-by:
    // `ast.Diagnostic` serialized-diagnostic surface (categories/keys/related).
    pub diagnostics: Option<serde_json::Value>,
}

impl BuildInfoSemanticDiagnostic {
    /// The bare-`fileId` form (file present, no cached diagnostics).
    pub fn file(file_id: BuildInfoFileId) -> Self {
        Self {
            file_id,
            diagnostics: None,
        }
    }
}

// Go: internal/execute/incremental/buildInfo.go:BuildInfoSemanticDiagnostic.MarshalJSON
impl Serialize for BuildInfoSemanticDiagnostic {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.file_id.0 != 0 {
            self.file_id.0.serialize(serializer)
        } else {
            self.diagnostics
                .clone()
                .unwrap_or(serde_json::Value::Null)
                .serialize(serializer)
        }
    }
}

// Go: internal/execute/incremental/buildInfo.go:BuildInfoSemanticDiagnostic.UnmarshalJSON
impl<'de> Deserialize<'de> for BuildInfoSemanticDiagnostic {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match serde_json::Value::deserialize(deserializer)? {
            serde_json::Value::Number(n) => Ok(BuildInfoSemanticDiagnostic::file(BuildInfoFileId(
                n.as_u64()
                    .ok_or_else(|| D::Error::custom("invalid semantic diagnostic fileId"))?
                    as u32,
            ))),
            other => Ok(Self {
                file_id: BuildInfoFileId(0),
                diagnostics: Some(other),
            }),
        }
    }
}

/// The serialized `.tsbuildinfo` payload (reachable subset).
///
/// Field order mirrors Go's `BuildInfo` struct so the serialized key order
/// matches `cmd/tsgo` byte-for-byte. Empty/zero fields are omitted (Go's
/// `omitzero`). Deferred fields (`emitDiagnosticsPerFile`, `changeFileSet`,
/// `affectedFilesPendingEmit`, `latestChangedDtsFile`, `emitSignatures`,
/// `resolvedRoot`, `semanticErrors`) are not modeled yet.
// Go: internal/execute/incremental/buildInfo.go:BuildInfo
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub errors: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub check_pending: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub root: Vec<BuildInfoRoot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_infos: Vec<BuildInfoFileInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_ids_list: Vec<Vec<BuildInfoFileId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<IndexMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referenced_map: Vec<BuildInfoReferenceMapEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub semantic_diagnostics_per_file: Vec<BuildInfoSemanticDiagnostic>,
}

impl BuildInfo {
    /// Whether this build info was written by the current compiler version.
    // Go: internal/execute/incremental/buildInfo.go:BuildInfo.IsValidVersion
    pub fn is_valid_version(&self) -> bool {
        self.version == tsgo_core::version::version()
    }

    /// Whether this is an incremental-program build info (has `fileNames`).
    // Go: internal/execute/incremental/buildInfo.go:BuildInfo.IsIncremental
    pub fn is_incremental(&self) -> bool {
        !self.file_names.is_empty()
    }

    /// The 1-based file name for `file_id`.
    // Go: internal/execute/incremental/buildInfo.go:BuildInfo.fileName
    pub fn file_name(&self, file_id: BuildInfoFileId) -> &str {
        &self.file_names[(file_id.0 - 1) as usize]
    }

    /// The 1-based file info for `file_id`.
    // Go: internal/execute/incremental/buildInfo.go:BuildInfo.fileInfo
    pub fn file_info(&self, file_id: BuildInfoFileId) -> &BuildInfoFileInfo {
        &self.file_infos[(file_id.0 - 1) as usize]
    }

    /// The stored content `version` of the file named `file_name`, if present.
    ///
    /// Used by the up-to-date check to decide whether a newer-mtime input
    /// actually changed text. Go resolves this via `BuildInfoRootInfoReader`
    /// over the root files; the reachable subset looks the name up directly in
    /// `fileNames`.
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/incremental/buildInfo.go:BuildInfoRootInfoReader.GetBuildInfoFileInfo
    pub fn version_of(&self, file_name: &str) -> Option<String> {
        let idx = self.file_names.iter().position(|n| n == file_name)?;
        Some(self.file_infos.get(idx)?.get_file_info().version)
    }

    /// Reconstructs the [`ReferenceMap`](crate::ReferenceMap) from this build
    /// info's `referencedMap`/`fileIdsList`, keyed by raw `fileNames` strings.
    ///
    /// Real builds key the reference map by resolved [`tspath::Path`](tsgo_tspath::Path)
    /// values (see Go `buildInfoToSnapshot`, which roots each name against the
    /// buildinfo directory or the default-lib path); the graph structure is the
    /// same. This name-keyed view is what the affected-files walk consumes when
    /// seeding reuse directly from a parsed `.tsbuildinfo`.
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/incremental/buildinfotosnapshot.go:toSnapshot.setReferencedMap
    pub fn reference_map_by_name(&self) -> crate::ReferenceMap {
        let mut map = crate::ReferenceMap::new();
        for entry in &self.referenced_map {
            let source = tsgo_tspath::Path(self.file_name(entry.file_id).to_string());
            let list = &self.file_ids_list[(entry.file_id_list_id.0 - 1) as usize];
            let refs = tsgo_collections::Set::from_items(
                list.iter()
                    .map(|&id| tsgo_tspath::Path(self.file_name(id).to_string())),
            );
            map.store_references(source, refs);
        }
        map
    }
}

fn json_u32<'de, D: Deserializer<'de>>(v: &serde_json::Value) -> Result<u32, D::Error> {
    v.as_u64()
        .map(|n| n as u32)
        .ok_or_else(|| D::Error::custom("expected a file id number"))
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

#[cfg(test)]
#[path = "build_info_test.rs"]
mod tests;
