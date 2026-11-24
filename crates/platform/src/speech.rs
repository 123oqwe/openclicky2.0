use openclicky_core::error::Result;
use openclicky_core::ports::{SpeechHandle, SpeechOptions, SpeechOut};

use crate::tts::{ChildHandle, OsSpeech};

pub struct SystemSpeech;

impl SpeechOut for SystemSpeech {
    fn speak(&self, text: &str, opts: &SpeechOptions) -> Result<Box<dyn SpeechHandle>> {
        let engine = OsSpeech::new(opts.speed, opts.voice.clone());
        let child = engine.spawn(text)?;
        Ok(Box::new(ChildHandle::new(child)))
    }
}
