//! Slug generation for workspace names.
//!
//! From reference/packages/core/src/util/slug.ts.

const ADJECTIVES: &[&str] = &[
    "brave", "calm", "clever", "cosmic", "crisp", "curious", "eager", "gentle", "glowing", "happy",
    "hidden", "jolly", "kind", "lucky", "mighty", "misty", "neon", "nimble", "playful", "proud",
    "quick", "quiet", "shiny", "silent", "stellar", "sunny", "swift", "tidy", "witty",
];

const NOUNS: &[&str] = &[
    "cabin", "cactus", "canyon", "circuit", "comet", "eagle", "engine", "falcon", "forest",
    "garden", "harbor", "island", "knight", "lagoon", "meadow", "moon", "mountain", "nebula",
    "orchid", "otter", "panda", "pixel", "planet", "river", "rocket", "sailor", "squid", "star",
    "tiger", "wizard", "wolf",
];

/// `Slug.create()` from the reference: `adjective-noun`.
pub fn create() -> String {
    let adjective = ADJECTIVES[rand_index(ADJECTIVES.len())];
    let noun = NOUNS[rand_index(NOUNS.len())];
    format!("{adjective}-{noun}")
}

fn rand_index(len: usize) -> usize {
    let uuid = uuid::Uuid::new_v4();
    uuid.as_bytes()[0] as usize % len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_format() {
        let slug = create();
        assert_eq!(slug.split('-').count(), 2);
        assert!(ADJECTIVES.contains(&slug.split('-').next().unwrap()));
        assert!(NOUNS.contains(&slug.split('-').nth(1).unwrap()));
    }
}
