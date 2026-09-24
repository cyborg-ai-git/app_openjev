/// Wall-clock timings for a complete non-streaming request, including retries.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct EMOpenjevHttpTiming {
    pub first_byte_ms: Option<u128>,
    pub total_ms: u128,
}
