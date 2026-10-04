//! The model manifest: the one list of the models Rift knows about. Orbit's tier says how much
//! memory this machine has, and the models directory says which of them are really on the drive.
//!
//! quasard picks what it runs from here, and the AI page shows the list.

use std::path::Path;

use serde::Deserialize;

use crate::ghost;

/// The part of the model manifest Quasar reads. Everything else in the file is ignored.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Manifest {
    /// Chat models, in the order the manifest lists them.
    #[serde(default)]
    pub chat: Vec<Chat>,
    /// Embedding models, for search by meaning.
    #[serde(default)]
    pub embedding: Vec<Embedding>,
    /// Voices, for saying words out loud.
    #[serde(default)]
    pub tts: Vec<Voice>,
    /// Speech models, for turning what was said into words.
    #[serde(default)]
    pub speech: Vec<Speech>,
}

/// One chat model from the manifest.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Chat {
    /// Short name. llama-server also answers to it on its api.
    pub id: String,
    /// File name under the models directory.
    pub file: String,
    /// How many parameters it has, as the people who trained it say it: `4B`, `30B-A3B`.
    #[serde(default)]
    pub parameters: String,
    /// How its weights are packed, for example `Q4_K_M`.
    #[serde(default)]
    pub quant: String,
    /// How much room the file takes, in gigabytes.
    #[serde(default)]
    pub size_gb: f64,
    /// The model to run when nothing says how big the machine is.
    #[serde(default)]
    pub default: bool,
    /// Only there for the boot test, so no tier ever asks for it.
    #[serde(default)]
    pub test: bool,
    /// What a machine needs to run it.
    pub tier: Needs,
}

impl Chat {
    /// What the model is, in one line: how many parameters, how they are packed and how much room
    /// the file takes. A manifest that says none of that gives an empty line.
    #[must_use]
    pub fn about(&self) -> String {
        let mut parts = Vec::new();
        if !self.parameters.is_empty() {
            parts.push(self.parameters.clone());
        }
        if !self.quant.is_empty() {
            parts.push(self.quant.clone());
        }
        if self.size_gb > 0.0 {
            parts.push(format!("{:.1} GB", self.size_gb));
        }
        parts.join(", ")
    }
}

/// One embedding model from the manifest.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Embedding {
    /// Short name.
    pub id: String,
    /// File name under the models directory.
    pub file: String,
    /// What goes in front of the words a person searches with.
    #[serde(default)]
    pub query_prefix: String,
    /// What goes in front of a text that is searched.
    #[serde(default)]
    pub document_prefix: String,
}

/// One voice from the manifest, for saying words out loud.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Voice {
    /// Short name.
    pub id: String,
    /// File name under the models directory.
    pub file: String,
    /// The phonemes the voice was trained on, in a file beside it.
    pub tokens: String,
}

/// One speech model from the manifest, for turning what was said into words.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Speech {
    /// Short name.
    pub id: String,
    /// File name under the models directory.
    pub file: String,
}

/// Memory a model needs, in gigabytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Needs {
    /// System memory.
    pub min_ram_gb: u32,
    /// Graphics memory. Quasar does not know a machine's yet, so a model that needs any is never
    /// picked.
    #[serde(default)]
    pub min_vram_gb: u32,
}

/// Orbit's AI tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Under 7 GiB of memory.
    Small,
    /// Under 15 GiB.
    Medium,
    /// 15 GiB and more.
    Large,
}

impl Tier {
    /// Every tier, smallest first, in the order Orbit lists them.
    pub const ALL: [Tier; 3] = [Self::Small, Self::Medium, Self::Large];

    /// Reads the word Orbit answers with.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "small" => Some(Self::Small),
            "medium" => Some(Self::Medium),
            "large" => Some(Self::Large),
            _ => None,
        }
    }

    /// The word, as Orbit says it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    /// The least memory a machine in the tier was sold with, in GB. Orbit draws its lines just
    /// under 8 and 16 GB because the kernel keeps some of it back.
    #[must_use]
    pub const fn ram_gb(self) -> u32 {
        match self {
            Self::Small => 4,
            Self::Medium => 8,
            Self::Large => 16,
        }
    }
}

/// The model that runs, and a line for the log that says why.
#[derive(Debug, PartialEq)]
pub struct Pick<'a> {
    /// The model.
    pub chat: &'a Chat,
    /// Why this one.
    pub reason: String,
}

impl Manifest {
    /// Parses the manifest.
    ///
    /// # Errors
    ///
    /// When the text is not TOML or a chat entry is missing a field.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// Reads and parses the manifest file.
    ///
    /// # Errors
    ///
    /// When the file cannot be read or does not parse.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{} does not parse: {e}", path.display()))
    }

    /// The model a tier asks for: the largest one it has the memory for. Without a tier, the
    /// manifest's default.
    #[must_use]
    pub fn wanted(&self, tier: Option<Tier>) -> Option<&Chat> {
        let Some(tier) = tier else {
            return self.chat.iter().find(|chat| chat.default);
        };
        self.chat
            .iter()
            .filter(|chat| !chat.test && fits(chat, tier.ram_gb()))
            .max_by_key(|chat| (chat.tier.min_ram_gb, chat.default))
    }

    /// Picks the model to run. A model named on the command line wins. Otherwise the tier's
    /// model if it is on the drive, and if it is not, the largest model on the drive that fits in
    /// the tier's memory. A model that does not fit never runs, it would only be killed.
    ///
    /// # Errors
    ///
    /// A sentence that says why nothing can run.
    pub fn pick(
        &self,
        tier: Option<Tier>,
        named: Option<&str>,
        on_drive: impl Fn(&str) -> bool,
    ) -> Result<Pick<'_>, String> {
        if let Some(name) = named {
            let chat = self
                .chat
                .iter()
                .find(|chat| chat.id == name || chat.file == name)
                .ok_or_else(|| format!("{name} is not a chat model in the manifest."))?;
            if !on_drive(&chat.file) {
                return Err(format!("{} is not on the drive.", chat.file));
            }
            return Ok(Pick {
                chat,
                reason: format!("running {}, as set", chat.id),
            });
        }

        let tier_name = tier.map_or("unknown", Tier::name);
        let wanted = self.wanted(tier);
        if let Some(chat) = wanted.filter(|chat| on_drive(&chat.file)) {
            return Ok(Pick {
                chat,
                reason: format!("tier {tier_name}, running {}", chat.id),
            });
        }
        let ram = tier.map_or_else(
            || wanted.map_or(0, |chat| chat.tier.min_ram_gb),
            Tier::ram_gb,
        );
        let fallback = self
            .chat
            .iter()
            .filter(|chat| fits(chat, ram) && on_drive(&chat.file))
            .max_by_key(|chat| chat.tier.min_ram_gb);
        match (wanted, fallback) {
            (Some(wanted), Some(chat)) => Ok(Pick {
                chat,
                reason: format!(
                    "tier {tier_name} wants {}, which is not on the drive, running {} instead",
                    wanted.id, chat.id
                ),
            }),
            (None, Some(chat)) => Ok(Pick {
                chat,
                reason: format!("tier {tier_name}, running {}", chat.id),
            }),
            (Some(wanted), None) => Err(why_not(ghost::on(), NO_CHAT, || {
                format!(
                    "No chat model that fits this machine is on the drive. It needs {}.",
                    wanted.file
                )
            })),
            (None, None) => Err(why_not(ghost::on(), NO_CHAT, || {
                "No chat model that fits this machine is on the drive.".into()
            })),
        }
    }
}

/// What a Ghost boot cannot do with a chat model, for the mode's own sentence.
const NO_CHAT: &str = "No model can answer a question";
/// The same for the voice.
const NO_VOICE: &str = "Words cannot be said out loud";
/// The same for speech.
const NO_SPEECH: &str = "Speech cannot be written down";
/// The same for search by meaning.
const NO_SEARCH: &str = "Search by meaning cannot run";

/// The sentence for a model that is not in the models directory. In Ghost mode the weights are on
/// the drive and out of reach, because persist stays locked and the models come off it, so saying
/// the drive has no model would be untrue: the mode is the reason and it says so (ADR-0084).
/// `missing` is what cannot be done, for the mode's own sentence; `otherwise` is what an ordinary
/// boot says, which names the file that is wanted.
fn why_not(in_ghost_mode: bool, missing: &str, otherwise: impl FnOnce() -> String) -> String {
    if in_ghost_mode {
        ghost::cannot(missing)
    } else {
        otherwise()
    }
}

fn fits(chat: &Chat, ram_gb: u32) -> bool {
    chat.tier.min_vram_gb == 0 && chat.tier.min_ram_gb <= ram_gb
}

/// Whether a model's weights are in the models directory. Everything that picks a model asks this,
/// and so does the page that lists them.
#[must_use]
pub fn on_drive(models_dir: &Path, file: &str) -> bool {
    models_dir.join(file).is_file()
}

impl Manifest {
    /// Picks the voice that says words out loud: the first one the manifest lists whose files are
    /// both on the drive.
    ///
    /// # Errors
    ///
    /// A sentence that says why nothing can say the words.
    pub fn pick_voice(&self, on_drive: impl Fn(&str) -> bool) -> Result<&Voice, String> {
        self.tts
            .iter()
            .find(|voice| on_drive(&voice.file) && on_drive(&voice.tokens))
            .ok_or_else(|| match self.tts.first() {
                Some(voice) => why_not(ghost::on(), NO_VOICE, || {
                    format!(
                        "Saying words out loud needs {}, which is not on the drive.",
                        voice.file
                    )
                }),
                None => "The model manifest has no voice.".into(),
            })
    }

    /// Picks the speech model: the first one the manifest lists that is on the drive. A larger
    /// one is more accurate and slower, and which is on the drive is the owner's choice.
    ///
    /// # Errors
    ///
    /// A sentence that says why nothing can turn speech into words.
    pub fn pick_speech(&self, on_drive: impl Fn(&str) -> bool) -> Result<&Speech, String> {
        self.speech
            .iter()
            .find(|model| on_drive(&model.file))
            .ok_or_else(|| match self.speech.first() {
                Some(model) => why_not(ghost::on(), NO_SPEECH, || {
                    format!(
                        "Turning speech into words needs {}, which is not on the drive.",
                        model.file
                    )
                }),
                None => "The model manifest has no speech model.".into(),
            })
    }

    /// Picks the embedding model to run: the first one the manifest lists that is on the drive.
    /// One is as good as another for a machine of any size, they are all small.
    ///
    /// # Errors
    ///
    /// A sentence that says why search by meaning cannot run.
    pub fn pick_embedding(&self, on_drive: impl Fn(&str) -> bool) -> Result<&Embedding, String> {
        self.embedding
            .iter()
            .find(|model| on_drive(&model.file))
            .ok_or_else(|| match self.embedding.first() {
                Some(model) => why_not(ghost::on(), NO_SEARCH, || {
                    format!(
                        "Search by meaning needs {}, which is not on the drive.",
                        model.file
                    )
                }),
                None => "The model manifest has no embedding model.".into(),
            })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_model_that_is_not_there_says_the_file_or_the_mode() {
        let ordinary = super::why_not(false, "No model can answer a question", || {
            "No chat model that fits this machine is on the drive. It needs small.gguf.".into()
        });
        assert_eq!(
            ordinary,
            "No chat model that fits this machine is on the drive. It needs small.gguf."
        );
        let ghost = super::why_not(true, "No model can answer a question", || {
            unreachable!("the file is not named in Ghost mode: the drive is locked, not empty")
        });
        assert_eq!(
            ghost,
            "No model can answer a question in Ghost mode: persist stays locked and nothing is \
             written to the drive."
        );
    }

    use super::*;

    const SAMPLE: &str = r#"
schema = 1

[[chat]]
id = "tiny"
file = "tiny.gguf"
url = "https://example.invalid/tiny.gguf"
test = true
tier = { min_ram_gb = 2, min_vram_gb = 0 }

[[chat]]
id = "small"
file = "small.gguf"
tier = { min_ram_gb = 4, min_vram_gb = 0 }

[[chat]]
id = "medium"
file = "medium.gguf"
default = true
tier = { min_ram_gb = 8, min_vram_gb = 0 }

[[chat]]
id = "gpu"
file = "gpu.gguf"
tier = { min_ram_gb = 8, min_vram_gb = 12 }

[[chat]]
id = "large"
file = "large.gguf"
tier = { min_ram_gb = 16, min_vram_gb = 0 }

[[embedding]]
id = "embed"
file = "embed.gguf"
"#;

    fn sample() -> Manifest {
        Manifest::parse(SAMPLE).unwrap()
    }

    fn drive(files: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |file| files.contains(&file)
    }

    #[test]
    fn the_manifest_parses_and_the_rest_is_ignored() {
        let manifest = sample();
        assert_eq!(manifest.chat.len(), 5);
        assert!(manifest.chat[0].test);
        assert!(manifest.chat[2].default);
        assert_eq!(manifest.chat[3].tier.min_vram_gb, 12);
        assert_eq!(manifest.embedding.len(), 1);
        assert_eq!(manifest.embedding[0].query_prefix, "");
        assert!(Manifest::parse("[[chat]]\nid = \"x\"\n").is_err());
    }

    #[test]
    fn the_first_embedding_model_on_the_drive_runs() {
        let manifest = Manifest::parse(
            "[[embedding]]\nid = \"a\"\nfile = \"a.gguf\"\n\n\
             [[embedding]]\nid = \"b\"\nfile = \"b.gguf\"\n",
        )
        .unwrap();
        let id = |files| {
            manifest
                .pick_embedding(drive(files))
                .map(|model| model.id.as_str())
        };
        assert_eq!(id(&["a.gguf", "b.gguf"]), Ok("a"));
        assert_eq!(id(&["b.gguf"]), Ok("b"));
        assert_eq!(
            id(&[]),
            Err("Search by meaning needs a.gguf, which is not on the drive.".to_string())
        );
        assert!(Manifest::default().pick_embedding(|_| true).is_err());
    }

    #[test]
    fn a_voice_runs_when_both_its_files_are_on_the_drive() {
        let manifest = Manifest::parse(
            "[[tts]]\nid = \"a\"\nfile = \"a.onnx\"\ntokens = \"a.tokens.txt\"\n\n\
             [[tts]]\nid = \"b\"\nfile = \"b.onnx\"\ntokens = \"b.tokens.txt\"\n",
        )
        .unwrap();
        let id = |files| {
            manifest
                .pick_voice(drive(files))
                .map(|voice| voice.id.as_str())
        };
        assert_eq!(id(&["a.onnx", "a.tokens.txt", "b.onnx"]), Ok("a"));
        assert_eq!(id(&["a.onnx", "b.onnx", "b.tokens.txt"]), Ok("b"));
        assert_eq!(
            id(&["a.onnx"]),
            Err("Saying words out loud needs a.onnx, which is not on the drive.".to_string())
        );
        assert!(Manifest::default().pick_voice(|_| true).is_err());
    }

    #[test]
    fn the_first_speech_model_on_the_drive_is_the_one() {
        let manifest = Manifest::parse(
            "[[speech]]\nid = \"base\"\nfile = \"base.bin\"\n\n\
             [[speech]]\nid = \"turbo\"\nfile = \"turbo.bin\"\n",
        )
        .unwrap();
        let id = |files| {
            manifest
                .pick_speech(drive(files))
                .map(|model| model.id.as_str())
        };
        assert_eq!(id(&["base.bin", "turbo.bin"]), Ok("base"));
        assert_eq!(id(&["turbo.bin"]), Ok("turbo"));
        assert_eq!(
            id(&[]),
            Err("Turning speech into words needs base.bin, which is not on the drive.".to_string())
        );
        assert!(Manifest::default().pick_speech(|_| true).is_err());
    }

    #[test]
    fn tiers_read_as_orbit_writes_them() {
        for tier in Tier::ALL {
            assert_eq!(Tier::parse(tier.name()), Some(tier));
        }
        assert_eq!(Tier::ALL.map(Tier::name), ["small", "medium", "large"]);
        assert_eq!(Tier::ALL.map(Tier::ram_gb), [4, 8, 16]);
        assert_eq!(Tier::parse("huge"), None);
    }

    #[test]
    fn a_model_says_what_it_is_in_one_line() {
        let manifest = Manifest::parse(include_str!("../../../models/manifest.toml")).unwrap();
        let about = |id: &str| {
            manifest
                .chat
                .iter()
                .find(|chat| chat.id == id)
                .map(Chat::about)
        };
        assert_eq!(
            about("qwen3-4b-q4_k_m").as_deref(),
            Some("4B, Q4_K_M, 2.5 GB")
        );
        assert_eq!(
            about("qwen3-0.6b-q8_0").as_deref(),
            Some("0.6B, Q8_0, 0.6 GB")
        );
        // a manifest that says none of it says nothing rather than a line of commas
        let bare =
            Manifest::parse("[[chat]]\nid = \"x\"\nfile = \"x.gguf\"\ntier = { min_ram_gb = 4 }\n")
                .unwrap();
        assert_eq!(bare.chat[0].about(), "");
    }

    #[test]
    fn a_model_is_on_the_drive_when_its_file_is() {
        let dir = std::env::temp_dir().join(format!("rift-models-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("here.gguf");
        assert!(!on_drive(&dir, "here.gguf"));
        std::fs::write(&file, b"weights").unwrap();
        assert!(on_drive(&dir, "here.gguf"));
        assert!(!on_drive(&dir, "gone.gguf"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_tier_wants_the_largest_model_it_has_the_memory_for() {
        let manifest = sample();
        let wanted = |tier| manifest.wanted(tier).map(|chat| chat.id.as_str());
        assert_eq!(wanted(Some(Tier::Small)), Some("small"));
        assert_eq!(wanted(Some(Tier::Medium)), Some("medium"));
        assert_eq!(wanted(Some(Tier::Large)), Some("large"));
        assert_eq!(wanted(None), Some("medium"));
    }

    #[test]
    fn a_test_model_is_never_wanted() {
        let only_test = Manifest::parse(
            "[[chat]]\nid = \"t\"\nfile = \"t.gguf\"\ntest = true\ntier = { min_ram_gb = 2 }\n",
        )
        .unwrap();
        assert_eq!(only_test.wanted(Some(Tier::Large)), None);
    }

    #[test]
    fn the_wanted_model_runs_when_it_is_on_the_drive() {
        let manifest = sample();
        let every = drive(&["tiny.gguf", "small.gguf", "medium.gguf", "large.gguf"]);
        let pick = manifest.pick(Some(Tier::Medium), None, &every).unwrap();
        assert_eq!(pick.chat.id, "medium");
        assert_eq!(pick.reason, "tier medium, running medium");
    }

    #[test]
    fn without_it_the_largest_model_that_fits_runs() {
        let manifest = sample();
        let pick = manifest
            .pick(Some(Tier::Small), None, drive(&["tiny.gguf", "large.gguf"]))
            .unwrap();
        assert_eq!(pick.chat.id, "tiny");
        assert_eq!(
            pick.reason,
            "tier small wants small, which is not on the drive, running tiny instead"
        );
        let pick = manifest
            .pick(
                Some(Tier::Large),
                None,
                drive(&["small.gguf", "medium.gguf"]),
            )
            .unwrap();
        assert_eq!(pick.chat.id, "medium");
    }

    #[test]
    fn a_model_the_machine_cannot_carry_never_runs() {
        let manifest = sample();
        let err = manifest
            .pick(Some(Tier::Small), None, drive(&["large.gguf", "gpu.gguf"]))
            .unwrap_err();
        assert_eq!(
            err,
            "No chat model that fits this machine is on the drive. It needs small.gguf."
        );
        assert!(manifest.pick(None, None, drive(&[])).is_err());
    }

    #[test]
    fn a_named_model_wins_if_it_is_there() {
        let manifest = sample();
        let on_drive = drive(&["tiny.gguf", "large.gguf"]);
        let pick = manifest
            .pick(Some(Tier::Small), Some("large.gguf"), &on_drive)
            .unwrap();
        assert_eq!(pick.chat.id, "large");
        assert_eq!(pick.reason, "running large, as set");
        let pick = manifest.pick(None, Some("tiny"), &on_drive).unwrap();
        assert_eq!(pick.chat.id, "tiny");
        assert_eq!(
            manifest.pick(None, Some("medium"), &on_drive).unwrap_err(),
            "medium.gguf is not on the drive."
        );
        assert!(manifest.pick(None, Some("nope"), &on_drive).is_err());
    }

    #[test]
    fn the_real_manifest_follows_the_tier_table() {
        let manifest = Manifest::parse(include_str!("../../../models/manifest.toml")).unwrap();
        let wanted = |tier| manifest.wanted(tier).map(|chat| chat.id.as_str());
        assert_eq!(wanted(Some(Tier::Small)), Some("qwen3-1.7b-q4_k_m"));
        assert_eq!(wanted(Some(Tier::Medium)), Some("qwen3-4b-q4_k_m"));
        assert_eq!(wanted(Some(Tier::Large)), Some("qwen3-8b-q4_k_m"));
        assert_eq!(wanted(None), Some("qwen3-4b-q4_k_m"));

        // the boot test: a 4 GB vm with only the test model in @models
        let tests: Vec<_> = manifest.chat.iter().filter(|chat| chat.test).collect();
        assert_eq!(tests.len(), 1);
        let file = tests[0].file.clone();
        let pick = manifest
            .pick(Some(Tier::Small), None, move |f| f == file)
            .unwrap();
        assert_eq!(pick.chat.id, "qwen3-0.6b-q8_0");

        // search by meaning, with the words nomic wants in front of each side
        let embedding = manifest.pick_embedding(|_| true).unwrap();
        assert_eq!(embedding.id, "nomic-embed-text-v1.5-q8");
        assert_eq!(embedding.query_prefix, "search_query: ");
        assert_eq!(embedding.document_prefix, "search_document: ");

        // the voice that says words out loud, and the phonemes beside it
        let voice = manifest.pick_voice(|_| true).unwrap();
        assert_eq!(voice.id, "piper-en-us-lessac-medium");
        assert_eq!(voice.file, "en_US-lessac-medium.onnx");
        assert_eq!(voice.tokens, "en_US-lessac-medium.tokens.txt");

        // the speech model that turns what was said into words
        let speech = manifest.pick_speech(|_| true).unwrap();
        assert_eq!(speech.id, "whisper-base");
        assert_eq!(speech.file, "ggml-base.bin");
        assert_eq!(
            manifest
                .pick_speech(|file| file == "ggml-large-v3-turbo.bin")
                .unwrap()
                .id,
            "whisper-large-v3-turbo"
        );
    }
}
