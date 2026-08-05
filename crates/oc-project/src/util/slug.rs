/// From reference/packages/core/src/util/slug.ts
use rand::Rng;

pub struct Slug;

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

impl Slug {
    pub fn create() -> String {
        let mut rng = rand::thread_rng();
        let adjective = ADJECTIVES[rng.gen_range(0..ADJECTIVES.len())];
        let noun = NOUNS[rng.gen_range(0..NOUNS.len())];
        format!("{adjective}-{noun}")
    }
}
