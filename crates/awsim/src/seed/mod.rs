//! Bulk-seed helpers — hit `/_awsim/seed/<service>` to populate a
//! service with realistic fake data without going through SigV4.
//! Each submodule owns one service's writer + a small request/response
//! shape; this file holds the cross-service helpers (random names /
//! emails / lorem) so the writers stay tight.

use fake::Fake;
use fake::faker::internet::en::SafeEmail;
use fake::faker::lorem::en::{Sentence, Word};
use fake::faker::name::en::Name;
use rand::Rng;
use rand::seq::SliceRandom;

pub mod cognito;
pub mod dynamodb;

/// A single random first-name + last-name combo, e.g. "Alice Smith".
pub fn fake_name() -> String {
    Name().fake()
}

/// A safe (non-deliverable) random email like `user.name@example.com`.
pub fn fake_email() -> String {
    SafeEmail().fake()
}

/// One random lower-case word.
#[allow(dead_code)] // wired in upcoming s3/sqs seeders
pub fn fake_word() -> String {
    Word().fake()
}

/// Short random sentence (3-7 words) — useful as a bio / description.
#[allow(dead_code)] // wired in upcoming s3/secrets seeders
pub fn fake_sentence() -> String {
    Sentence(3..7).fake()
}

/// Build an identifier from `n` random words separated by `-`.
/// Useful for bucket names, queue names, table names, etc.
#[allow(dead_code)] // wired in upcoming bucket/queue/table seeders
pub fn fake_slug(n: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..n)
        .map(|_| Word().fake::<String>())
        .collect::<Vec<_>>()
        .join("-")
        + "-"
        + &rng.gen_range(100..999).to_string()
}

/// Pick one random element from a slice; falls back to the first if
/// the slice is empty (won't panic).
pub fn pick<T>(items: &[T]) -> &T {
    let mut rng = rand::thread_rng();
    items.choose(&mut rng).unwrap_or(&items[0])
}

/// Roll a coin with the supplied probability of true (0..1). Keeps
/// `if probability(0.85) { ... }` calls tidy at the call site.
pub fn probability(p: f64) -> bool {
    rand::thread_rng().gen_bool(p.clamp(0.0, 1.0))
}
