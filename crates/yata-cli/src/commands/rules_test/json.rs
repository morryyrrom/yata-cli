//! The JSON output of `yata rules test`.

use std::io::{self, Write};

use serde::Serialize;
use yata_engine::testing::TestReport;

/// Version of this output format. Raised only for incompatible changes (removed or redefined
/// members); added members do not change it.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct Tool {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct Output<'a> {
    schema_version: u32,
    tool: Tool,
    #[serde(flatten)]
    report: &'a TestReport,
}

/// Writes the report as pretty-printed JSON followed by a newline: `schema_version` and `tool`
/// first, then the members of [`TestReport`].
pub fn write(out: &mut dyn Write, report: &TestReport) -> io::Result<()> {
    let output = Output {
        schema_version: SCHEMA_VERSION,
        tool: Tool {
            name: "yata",
            version: env!("CARGO_PKG_VERSION"),
        },
        report,
    };
    serde_json::to_writer_pretty(&mut *out, &output).map_err(io::Error::from)?;
    out.write_all(b"\n")
}
