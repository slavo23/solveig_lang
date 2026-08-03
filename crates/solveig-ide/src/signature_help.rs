//! Source-level signature help over retained checked expressions.

use super::position::{position_index_for_target, target_at_position_in_artifacts};
use super::*;

pub(super) fn signature_help_at_position_in_artifacts(
    artifacts: &CompilationArtifacts,
    source_map: &SourceMap,
    file: &Path,
    position: SourcePosition,
    encoding: PositionEncoding,
) -> Option<SourceSignatureHelp> {
    let target = target_at_position_in_artifacts(artifacts, source_map, file, position, encoding)?;
    position_index_for_target(artifacts, target)?.signature_help(target)
}
