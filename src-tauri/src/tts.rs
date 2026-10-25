use edge_tts_rust::{EdgeTtsClient, SpeakOptions};
use lazy_static::lazy_static;
use regex::Regex;
use rodio::Decoder;
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex as StdMutex;
use std::thread;

lazy_static! {
    static ref AUDIO_SENDER: StdMutex<Option<Sender<AudioCommand>>> = StdMutex::new(None);
    // static ref KOKORO_ENGINE: TokioMutex<Option<(String, String, TtsEngine)>> = TokioMutex::new(None);
}

#[allow(dead_code)]
enum AudioCommand {
    PlayRawPcm {
        samples: Vec<f32>,
        channels: u16,
        sample_rate: u32,
    },
    PlayEncodedBytes(Vec<u8>),
    Stop,
}

fn ensure_audio_thread() {
    let mut sender = AUDIO_SENDER.lock().unwrap();
    if sender.is_none() {
        println!("[AUDIO THREAD] Initializing audio thread...");
        let (tx, rx) = channel::<AudioCommand>();
        *sender = Some(tx);
        thread::spawn(
            move || match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(handle) => {
                    println!("[AUDIO THREAD] Audio device opened successfully");
                    let mut current_player: Option<rodio::Player> = None;
                    for cmd in rx {
                        match cmd {
                            AudioCommand::PlayRawPcm {
                                samples,
                                channels,
                                sample_rate,
                            } => {
                                println!(
                                    "[AUDIO THREAD] PlayRawPcm command received ({} samples, {}Hz)",
                                    samples.len(),
                                    sample_rate
                                );
                                let player = rodio::Player::connect_new(&handle.mixer());
                                player.set_volume(1.0);
                                let ch = NonZeroU16::new(channels)
                                    .unwrap_or(NonZeroU16::new(1).unwrap());
                                let sr = NonZeroU32::new(sample_rate)
                                    .unwrap_or(NonZeroU32::new(24000).unwrap());
                                let source = rodio::buffer::SamplesBuffer::new(ch, sr, samples);
                                player.append(source);
                                player.play();
                                current_player = Some(player);
                                println!(
                                    "[AUDIO THREAD] Audio appended to player, starting playback..."
                                );
                            }
                            AudioCommand::PlayEncodedBytes(bytes) => {
                                println!(
                                    "[AUDIO THREAD] PlayEncodedBytes command received ({} bytes)",
                                    bytes.len()
                                );
                                let cursor = std::io::Cursor::new(bytes);
                                match Decoder::new(cursor) {
                                    Ok(source) => {
                                        println!("[AUDIO THREAD] Decoder ready");
                                        let player = rodio::Player::connect_new(&handle.mixer());
                                        player.set_volume(1.0);
                                        player.append(source);
                                        player.play();
                                        current_player = Some(player);
                                        println!("[AUDIO THREAD] Audio appended to player, starting playback...");
                                    }
                                    Err(e) => {
                                        println!("[AUDIO THREAD] Decoder error: {}", e);
                                    }
                                }
                            }
                            AudioCommand::Stop => {
                                println!("[AUDIO THREAD] Stop command received");
                                if let Some(player) = current_player.take() {
                                    player.stop();
                                    println!("[AUDIO THREAD] Player stopped");
                                }
                            }
                        }
                    }
                    println!("[AUDIO THREAD] Channel closed, audio thread exiting");
                }
                Err(e) => {
                    println!(
                        "[AUDIO THREAD] CRITICAL: Failed to open audio device: {}",
                        e
                    );
                    println!("[AUDIO THREAD] No audio output will work!");
                }
            },
        );
        println!("[AUDIO THREAD] Audio thread spawned");
    } else {
        println!("[AUDIO THREAD] Audio thread already exists");
    }
}

#[tauri::command]
pub async fn tts_speak(text: String, voice: Option<String>) -> Result<String, String> {
    println!("[CLOUD TTS] Starting Azure EdgeTTS synthesis...");
    println!("  Text: \"{}\"", text);
    println!("  Voice: {:?}", voice);

    ensure_audio_thread();

    let client = EdgeTtsClient::new().map_err(|e| {
        let err_msg = format!("EdgeTTS client creation failed: {}", e);
        println!("{}", err_msg);
        err_msg
    })?;

    let voice_name = voice.unwrap_or_else(|| "hi-IN-SwaraNeural".to_string());
    let voice_name = if voice_name.is_empty() {
        "hi-IN-SwaraNeural".to_string()
    } else {
        voice_name
    };
    println!("[CLOUD TTS] Using voice: {}", voice_name);

    let options = SpeakOptions {
        voice: voice_name,
        ..Default::default()
    };

    let re = Regex::new(r"[*`_~#]").unwrap();
    let clean_text = re.replace_all(&text, "").to_string();
    println!("[CLOUD TTS] Cleaned text: \"{}\"", clean_text);

    let res = client.synthesize(clean_text, options).await.map_err(|e| {
        let err_msg = format!("EdgeTTS synthesis failed: {}", e);
        println!("{}", err_msg);
        err_msg
    })?;

    println!("[CLOUD TTS] Synthesis OK ({} bytes)", res.audio.len());

    if let Some(sender) = AUDIO_SENDER.lock().unwrap().as_ref() {
        let _ = sender.send(AudioCommand::PlayEncodedBytes(res.audio.clone()));
    }

    // Eliminates duplicate playback: host audio sink is authoritative.
    println!("[CLOUD TTS] Done (audio playing natively on host soundcard)!");
    Ok("".to_string())
}



#[tauri::command]
pub async fn tts_stop() -> Result<(), String> {
    println!("⏹️ [AUDIO] Stop command received");
    if let Some(sender) = AUDIO_SENDER.lock().unwrap().as_ref() {
        match sender.send(AudioCommand::Stop) {
            Ok(_) => println!("⏹️ [AUDIO] ✅ Stop command sent"),
            Err(e) => println!("⏹️ [AUDIO] ❌ Failed to send stop command: {}", e),
        }
    } else {
        println!("⏹️ [AUDIO] ❌ No audio sender available!");
    }
    Ok(())
}

#[tauri::command]
pub async fn tts_set_voice(_voice: String) -> Result<(), String> {
    Ok(())
}
