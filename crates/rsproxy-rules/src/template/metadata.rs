use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static FALLBACK_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
/// Lazily generated values that remain stable across both phases of one request.
///
/// Clones share the same initialization cell. Generated IDs are 32 lowercase
/// hexadecimal characters and generated UUIDs are RFC 4122 version 4 values.
pub struct TemplateMetadata {
    values: Arc<OnceLock<TemplateValues>>,
}

#[derive(Debug, PartialEq, Eq)]
struct TemplateValues {
    id: String,
    now_ms: u64,
    random: u64,
    random_uuid: String,
}

impl TemplateMetadata {
    /// Generates all metadata immediately, using OS entropy with a deterministic fallback.
    pub fn generate() -> Self {
        Self::from_values(generate_values())
    }

    fn from_values(values: TemplateValues) -> Self {
        let cell = OnceLock::new();
        cell.set(values)
            .expect("new template metadata cell is uninitialized");
        Self {
            values: Arc::new(cell),
        }
    }

    fn values(&self) -> &TemplateValues {
        self.values.get_or_init(generate_values)
    }

    /// Creates deterministic metadata for replay, tests, or an externally assigned request id.
    pub fn fixed(
        id: impl Into<String>,
        now_ms: u64,
        random: u64,
        random_uuid: impl Into<String>,
    ) -> Self {
        Self::from_values(TemplateValues {
            id: id.into(),
            now_ms,
            random,
            random_uuid: random_uuid.into(),
        })
    }

    /// Returns the request identifier exposed as `${id}`.
    pub fn id(&self) -> &str {
        &self.values().id
    }

    /// Returns the Unix-millisecond timestamp exposed as `${now}`.
    pub fn now_ms(&self) -> u64 {
        self.values().now_ms
    }

    /// Returns the per-request unsigned value exposed as `${random}`.
    pub fn random(&self) -> u64 {
        self.values().random
    }

    /// Returns the per-request UUID string exposed as `${randomUUID}`.
    pub fn random_uuid(&self) -> &str {
        &self.values().random_uuid
    }
}

fn generate_values() -> TemplateValues {
    let now_ms = now_millis();
    let mut entropy = [0u8; 40];
    if getrandom::fill(&mut entropy).is_err() {
        fill_fallback_entropy(&mut entropy, now_ms);
    }
    let random = u64::from_le_bytes(
        entropy[16..24]
            .try_into()
            .expect("eight-byte entropy slice must form a u64"),
    );
    entropy[24 + 6] = (entropy[24 + 6] & 0x0f) | 0x40;
    entropy[24 + 8] = (entropy[24 + 8] & 0x3f) | 0x80;
    TemplateValues {
        id: encode_hex(&entropy[..16]),
        now_ms,
        random,
        random_uuid: encode_uuid(&entropy[24..40]),
    }
}

impl Default for TemplateMetadata {
    fn default() -> Self {
        Self {
            values: Arc::new(OnceLock::new()),
        }
    }
}

impl PartialEq for TemplateMetadata {
    fn eq(&self, other: &Self) -> bool {
        self.values() == other.values()
    }
}

impl Eq for TemplateMetadata {}

pub(crate) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn fill_fallback_entropy(output: &mut [u8], now_ms: u64) {
    let counter = FALLBACK_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut state = now_ms ^ counter.rotate_left(23) ^ (std::process::id() as u64);
    for chunk in output.chunks_mut(8) {
        state = splitmix64(state);
        chunk.copy_from_slice(&state.to_le_bytes()[..chunk.len()]);
    }
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}")
            .expect("formatting hexadecimal bytes into a String cannot fail");
    }
    output
}

fn encode_uuid(bytes: &[u8]) -> String {
    let encoded = encode_hex(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &encoded[..8],
        &encoded[8..12],
        &encoded[12..16],
        &encoded[16..20],
        &encoded[20..32]
    )
}
