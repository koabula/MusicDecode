use aes::{
    cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit},
    Aes128,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use id3::{
    frame::{Picture, PictureType},
    Tag, TagLike, Version,
};
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use tempfile::{Builder, TempPath};

const CORE: &[u8; 16] = b"hzHRAmso5kInbaxW";
const META: &[u8; 16] = b"#14ljk_!\\]&0U<'(";

pub fn display_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(unc) = s.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        s.strip_prefix("\\\\?\\").unwrap_or(&s).to_owned()
    }
}

pub struct Info {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub format: String,
    pub duration: u64,
    pub bitrate: u64,
}

struct Header {
    metadata: Value,
    cover: Vec<u8>,
    box_key: [u8; 256],
    audio_length: u64,
    format: String,
}

fn ioerr(e: impl std::fmt::Display) -> String {
    format!("文件读写失败：{e}")
}
fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        Err("已取消".into())
    } else {
        Ok(())
    }
}

fn read_u32(file: &mut File) -> Result<u32, String> {
    let mut b = [0; 4];
    file.read_exact(&mut b)
        .map_err(|_| "文件不完整：缺少长度字段")?;
    Ok(u32::from_le_bytes(b))
}

fn read_block(file: &mut File, len: u32, max: u32) -> Result<Vec<u8>, String> {
    if len > max {
        return Err("NCM 数据长度异常或版本暂不支持".into());
    }
    let end = file
        .stream_position()
        .map_err(ioerr)?
        .checked_add(len as u64)
        .ok_or("数据长度溢出")?;
    if end > file.metadata().map_err(ioerr)?.len() {
        return Err("文件不完整：数据块被截断".into());
    }
    let mut bytes = vec![0; len as usize];
    file.read_exact(&mut bytes).map_err(ioerr)?;
    Ok(bytes)
}

fn aes_decrypt(bytes: &mut Vec<u8>, key: &[u8; 16]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() % 16 != 0 {
        return Err("NCM 密钥数据长度无效".into());
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    for chunk in bytes.chunks_exact_mut(16) {
        cipher.decrypt_block(GenericArray::from_mut_slice(chunk));
    }
    let padding = *bytes.last().unwrap() as usize;
    if padding == 0
        || padding > 16
        || !bytes[bytes.len() - padding..]
            .iter()
            .all(|&v| v as usize == padding)
    {
        return Err("无法解密：文件损坏或加密版本暂不支持".into());
    }
    bytes.truncate(bytes.len() - padding);
    Ok(())
}

fn make_box(key: &[u8]) -> [u8; 256] {
    let mut table = [0u8; 256];
    for (i, b) in table.iter_mut().enumerate() {
        *b = i as u8;
    }
    let mut j = 0;
    for i in 0..256 {
        j = (table[i] as usize + j + key[i % key.len()] as usize) & 255;
        table.swap(i, j);
    }
    table
}

fn decrypt_audio(bytes: &mut [u8], table: &[u8; 256], offset: u64) {
    for (i, b) in bytes.iter_mut().enumerate() {
        let j = ((offset + i as u64 + 1) & 255) as usize;
        *b ^= table[(table[j] as usize + table[(table[j] as usize + j) & 255] as usize) & 255];
    }
}

fn detect(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"ID3")
        || (bytes.len() > 2
            && bytes[0] == 255
            && bytes[1] & 0xe0 == 0xe0
            && bytes[1] & 0x18 != 0x08
            && bytes[1] & 6 != 0)
    {
        Some("MP3")
    } else if bytes.starts_with(b"fLaC") {
        Some("FLAC")
    } else if bytes.len() > 8 && &bytes[4..8] == b"ftyp" {
        Some("M4A")
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        Some("WAV")
    } else if bytes.starts_with(b"OggS") {
        Some("OGG")
    } else {
        None
    }
}

fn metadata(bytes: Vec<u8>) -> Option<Value> {
    if bytes.is_empty() {
        return None;
    }
    let unmasked: Vec<u8> = bytes.into_iter().map(|b| b ^ 0x63).collect();
    let encoded = unmasked.strip_prefix(b"163 key(Don't modify):")?;
    let mut plain = STANDARD.decode(encoded).ok()?;
    aes_decrypt(&mut plain, META).ok()?;
    serde_json::from_slice(plain.strip_prefix(b"music:")?).ok()
}

fn parse(file: &mut File) -> Result<Header, String> {
    let size = file.metadata().map_err(ioerr)?.len();
    let mut magic = [0; 10];
    file.read_exact(&mut magic)
        .map_err(|_| "文件过小，不是完整的 NCM 文件")?;
    if &magic[..8] != b"CTENFDAM" {
        return Err("文件内容不是 NCM，不能只修改扩展名".into());
    }
    let len = read_u32(file)?;
    let mut key = read_block(file, len, 64 * 1024)?;
    for b in &mut key {
        *b ^= 0x64;
    }
    aes_decrypt(&mut key, CORE)?;
    let key = key
        .strip_prefix(b"neteasecloudmusic")
        .filter(|k| !k.is_empty())
        .ok_or("NCM 音频密钥无效")?;
    let box_key = make_box(key);
    let len = read_u32(file)?;
    let metadata = metadata(read_block(file, len, 16 * 1024 * 1024)?).unwrap_or(Value::Null);
    let mut crc = [0; 5];
    file.read_exact(&mut crc)
        .map_err(|_| "文件不完整：缺少封面数据")?;
    let cover_frame = read_u32(file)?;
    let cover_len = read_u32(file)?;
    if cover_len > cover_frame {
        return Err("NCM 封面长度无效".into());
    }
    let cover = read_block(file, cover_len, 32 * 1024 * 1024)?;
    let offset = file
        .stream_position()
        .map_err(ioerr)?
        .checked_add((cover_frame - cover_len) as u64)
        .ok_or("封面长度溢出")?;
    if offset >= size {
        return Err("文件中没有完整的音频数据".into());
    }
    file.seek(SeekFrom::Start(offset)).map_err(ioerr)?;
    let mut first = vec![0; (size - offset).min(64) as usize];
    file.read_exact(&mut first).map_err(ioerr)?;
    decrypt_audio(&mut first, &box_key, 0);
    let format = detect(&first)
        .ok_or("无法识别解密后的音频：文件可能损坏或版本暂不支持")?
        .to_owned();
    file.seek(SeekFrom::Start(offset)).map_err(ioerr)?;
    Ok(Header {
        metadata,
        cover,
        box_key,
        audio_length: size - offset,
        format,
    })
}

fn artists(meta: &Value) -> Option<String> {
    let names: Vec<_> = meta["artist"]
        .as_array()?
        .iter()
        .filter_map(|a| a[0].as_str())
        .collect();
    if names.is_empty() {
        None
    } else {
        Some(names.join(" / "))
    }
}

pub fn inspect(path: &Path) -> Result<Info, String> {
    let mut file = File::open(path).map_err(ioerr)?;
    let h = parse(&mut file)?;
    Ok(Info {
        title: h.metadata["musicName"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        artist: artists(&h.metadata),
        format: h.format,
        duration: h.metadata["duration"].as_u64().unwrap_or(0),
        bitrate: h.metadata["bitrate"].as_u64().unwrap_or(0),
    })
}

fn temp(dir: &Path, suffix: &str) -> Result<TempPath, String> {
    Builder::new()
        .prefix(".musicdecode-")
        .suffix(suffix)
        .tempfile_in(dir)
        .map(|f| f.into_temp_path())
        .map_err(ioerr)
}

fn ffmpeg_run(
    ffmpeg: &Path,
    args: &[String],
    cancel: &AtomicBool,
    dir: &Path,
) -> Result<(), String> {
    let log = temp(dir, ".log")?;
    let error_file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&log)
        .map_err(ioerr)?;
    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-nostdin", "-v", "error", "-xerror"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(error_file));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动音频组件，请重新安装完整程序：{e}"))?;
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("已取消".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                let mut detail = String::new();
                if let Ok(file) = File::open(&log) {
                    let _ = file.take(8192).read_to_string(&mut detail);
                }
                let detail = detail
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .take(2)
                    .collect::<Vec<_>>()
                    .join("；");
                return Err(format!(
                    "音频处理失败{}",
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!("：{detail}")
                    }
                ));
            }
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ioerr(e));
            }
        }
        if started.elapsed() > Duration::from_secs(3600) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("音频处理超时，请检查文件".into());
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}

fn write_tags(path: &Path, h: &Header) -> Result<(), String> {
    let mut tag = id3::partial_tag_ok(Tag::read_from_path(path)).unwrap_or_default();
    if let Some(title) = h.metadata["musicName"].as_str().filter(|s| !s.is_empty()) {
        tag.set_title(title);
    }
    if let Some(artist) = artists(&h.metadata) {
        tag.set_artist(artist);
    }
    if let Some(album) = h.metadata["album"].as_str().filter(|s| !s.is_empty()) {
        tag.set_album(album);
    }
    if !h.cover.is_empty() {
        let mime = if h.cover.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some("image/png")
        } else if h.cover.starts_with(b"\xff\xd8\xff") {
            Some("image/jpeg")
        } else {
            None
        };
        if let Some(mime) = mime {
            tag.remove("APIC");
            tag.add_frame(Picture {
                mime_type: mime.into(),
                picture_type: PictureType::CoverFront,
                description: String::new(),
                data: h.cover.clone(),
            });
        }
    }
    tag.write_to_path(path, Version::Id3v23)
        .map_err(|e| format!("无法保存歌曲信息：{e}"))
}

fn publish(mut temp: TempPath, dir: &Path, stem: &str) -> Result<PathBuf, String> {
    for n in 0..10000 {
        let name = if n == 0 {
            format!("{stem}.mp3")
        } else {
            format!("{stem} ({n}).mp3")
        };
        let target = dir.join(name);
        match temp.persist_noclobber(&target) {
            Ok(()) => return Ok(target),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => temp = e.path,
            Err(e) => return Err(ioerr(e.error)),
        }
    }
    Err("同名文件过多，请选择其他输出目录".into())
}

pub fn convert<F: Fn(u8, &str)>(
    source: &Path,
    output: Option<&Path>,
    bitrate: u32,
    ffmpeg: &Path,
    cancel: &AtomicBool,
    progress: F,
) -> Result<PathBuf, String> {
    cancelled(cancel)?;
    if !ffmpeg.is_file() {
        return Err("找不到音频组件，请重新安装完整程序".into());
    }
    if ![192, 256, 320].contains(&bitrate) {
        return Err("不支持的转码码率".into());
    }
    let mut file = File::open(source).map_err(ioerr)?;
    let h = parse(&mut file)?;
    let dir = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| source.parent().unwrap_or(Path::new(".")).join("Decoded"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法写入输出目录：{e}"))?;
    let raw = temp(&dir, if h.format == "MP3" { ".mp3" } else { ".audio" })?;
    let mut writer = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&raw)
        .map_err(ioerr)?;
    let mut buffer = vec![0u8; 128 * 1024];
    let mut done = 0u64;
    let mut last_progress = 255;
    while done < h.audio_length {
        cancelled(cancel)?;
        let amount = buffer.len().min((h.audio_length - done) as usize);
        file.read_exact(&mut buffer[..amount])
            .map_err(|_| "音频数据被截断")?;
        decrypt_audio(&mut buffer[..amount], &h.box_key, done);
        writer.write_all(&buffer[..amount]).map_err(ioerr)?;
        done += amount as u64;
        let pct = (done.saturating_mul(65) / h.audio_length) as u8;
        if pct != last_progress {
            progress(pct, "正在解密");
            last_progress = pct;
        }
    }
    writer.flush().map_err(ioerr)?;
    drop(writer);
    drop(file);
    cancelled(cancel)?;
    let mp3 = if h.format == "MP3" {
        raw
    } else {
        progress(70, "正在转为 MP3");
        let mp3 = temp(&dir, ".mp3")?;
        ffmpeg_run(
            ffmpeg,
            &[
                "-y".into(),
                "-i".into(),
                display_path(&raw),
                "-map".into(),
                "0:a:0".into(),
                "-map_metadata".into(),
                "0".into(),
                "-codec:a".into(),
                "libmp3lame".into(),
                "-b:a".into(),
                format!("{bitrate}k"),
                "-f".into(),
                "mp3".into(),
                display_path(&mp3),
            ],
            cancel,
            &dir,
        )?;
        mp3
    };
    cancelled(cancel)?;
    progress(88, "正在保存歌曲信息");
    write_tags(&mp3, &h)?;
    cancelled(cancel)?;
    progress(94, "正在检查音频完整性");
    ffmpeg_run(
        ffmpeg,
        &[
            "-i".into(),
            display_path(&mp3),
            "-map".into(),
            "0:a:0".into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ],
        cancel,
        &dir,
    )?;
    cancelled(cancel)?;
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("music");
    publish(mp3, &dir, stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncrypt;
    use sha2::{Digest, Sha256};

    fn encrypt(mut bytes: Vec<u8>, key: &[u8; 16]) -> Vec<u8> {
        let n = 16 - bytes.len() % 16;
        bytes.extend(vec![n as u8; n]);
        let cipher = Aes128::new(GenericArray::from_slice(key));
        for b in bytes.chunks_exact_mut(16) {
            cipher.encrypt_block(GenericArray::from_mut_slice(b));
        }
        bytes
    }
    fn fixture(audio: &[u8], cover_reserve: usize, with_meta: bool) -> Vec<u8> {
        fixture_with_cover(audio, cover_reserve, with_meta, &[])
    }
    fn fixture_with_cover(
        audio: &[u8],
        cover_reserve: usize,
        with_meta: bool,
        cover: &[u8],
    ) -> Vec<u8> {
        let key = b"test-key-material";
        let enc = encrypt([b"neteasecloudmusic".as_slice(), key].concat(), CORE)
            .into_iter()
            .map(|b| b ^ 0x64)
            .collect::<Vec<_>>();
        let meta = if with_meta {
            let plain=b"music:{\"musicName\":\"fixture\",\"artist\":[[\"artist\",1]],\"format\":\"flac\"}".to_vec();
            let encoded = STANDARD.encode(encrypt(plain, META));
            format!("163 key(Don't modify):{encoded}")
                .bytes()
                .map(|b| b ^ 0x63)
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let mut out = b"CTENFDAM\x01\x70".to_vec();
        out.extend((enc.len() as u32).to_le_bytes());
        out.extend(enc);
        out.extend((meta.len() as u32).to_le_bytes());
        out.extend(meta);
        out.extend([0; 5]);
        let cover_frame = cover_reserve.max(cover.len());
        out.extend((cover_frame as u32).to_le_bytes());
        out.extend((cover.len() as u32).to_le_bytes());
        out.extend(cover);
        out.extend(vec![0; cover_frame - cover.len()]);
        let mut encrypted = audio.to_vec();
        decrypt_audio(&mut encrypted, &make_box(key), 0);
        out.extend(encrypted);
        out
    }
    #[test]
    fn chunk_boundaries_do_not_change_audio() {
        let original: Vec<u8> = (0..200_000).map(|i| (i % 251) as u8).collect();
        let table = make_box(b"test");
        let mut encrypted = original.clone();
        decrypt_audio(&mut encrypted, &table, 0);
        for (i, chunk) in encrypted.chunks_mut(7919).enumerate() {
            decrypt_audio(chunk, &table, (i * 7919) as u64);
        }
        assert_eq!(original, encrypted);
    }
    #[test]
    fn new_cover_padding_and_format_sniffing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("padding.ncm");
        std::fs::write(&p, fixture(b"ID3\x03\0\0sample-mp3", 8192, true)).unwrap();
        let mut file = File::open(&p).unwrap();
        let h = parse(&mut file).unwrap();
        assert_eq!(h.format, "MP3");
        assert_eq!(h.metadata["format"], "flac");
        assert!(h.cover.is_empty());
        let mut audio = Vec::new();
        file.read_to_end(&mut audio).unwrap();
        decrypt_audio(&mut audio, &h.box_key, 0);
        assert!(audio.starts_with(b"ID3"));
    }
    #[test]
    fn missing_metadata_is_supported() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("empty.ncm");
        std::fs::write(&p, fixture(b"fLaCstreaminfo", 0, false)).unwrap();
        let h = inspect(&p).unwrap();
        assert_eq!(h.format, "FLAC");
        assert!(h.title.is_none());
    }
    #[test]
    fn truncation_and_bad_magic_are_rejected() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bad.ncm");
        for bytes in [vec![0; 20], b"CTENFDAM\0\0\xff\xff\xff\xff".to_vec()] {
            std::fs::write(&p, bytes).unwrap();
            assert!(inspect(&p).is_err());
        }
    }
    #[test]
    fn publication_never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        let original = d.path().join("song.mp3");
        std::fs::write(&original, b"existing").unwrap();
        let t = temp(d.path(), ".mp3").unwrap();
        std::fs::write(&t, b"new").unwrap();
        let out = publish(t, d.path(), "song").unwrap();
        assert_eq!(out.file_name().unwrap(), "song (1).mp3");
        assert_eq!(std::fs::read(original).unwrap(), b"existing");
    }
    #[test]
    fn real_file_and_flac_conversion() {
        let Ok(real) = std::env::var("MUSICDECODE_TEST_NCM") else {
            return;
        };
        let ffmpeg =
            PathBuf::from(std::env::var("MUSICDECODE_TEST_FFMPEG").expect("set ffmpeg path"));
        let d = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let source = Path::new(&real);
        let before = std::fs::read(source).unwrap();
        let mut input = File::open(source).unwrap();
        let h = parse(&mut input).unwrap();
        let mut audio = Vec::new();
        input.read_to_end(&mut audio).unwrap();
        decrypt_audio(&mut audio, &h.box_key, 0);
        let out = convert(source, Some(d.path()), 320, &ffmpeg, &cancel, |_, _| {}).unwrap();
        fn payload(bytes: &[u8]) -> &[u8] {
            if bytes.starts_with(b"ID3") {
                let n = ((bytes[6] as usize) << 21)
                    | ((bytes[7] as usize) << 14)
                    | ((bytes[8] as usize) << 7)
                    | bytes[9] as usize;
                &bytes[10 + n..]
            } else {
                bytes
            }
        }
        let result = std::fs::read(&out).unwrap();
        assert_eq!(
            Sha256::digest(payload(&audio)),
            Sha256::digest(payload(&result))
        );
        assert_eq!(before, std::fs::read(source).unwrap());
        let tag = Tag::read_from_path(&out).unwrap();
        assert_eq!(tag.title(), Some("Starry Starry Night"));
        assert_eq!(tag.pictures().count(), 1);
        let second = convert(source, Some(d.path()), 320, &ffmpeg, &cancel, |_, _| {}).unwrap();
        assert_ne!(out, second);
        cancel.store(true, Ordering::SeqCst);
        assert!(convert(source, Some(d.path()), 320, &ffmpeg, &cancel, |_, _| {}).is_err());
        cancel.store(false, Ordering::SeqCst);
        let inflight = d.path().join("cancel-mid-stream");
        assert!(
            convert(source, Some(&inflight), 320, &ffmpeg, &cancel, |pct, _| {
                if pct > 0 {
                    cancel.store(true, Ordering::SeqCst);
                }
            })
            .is_err()
        );
        assert_eq!(std::fs::read_dir(inflight).unwrap().count(), 0);
        cancel.store(false, Ordering::SeqCst);
    }
    #[test]
    fn synthetic_conversion_pipeline() {
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let ffmpeg = std::env::var_os("MUSICDECODE_TEST_FFMPEG")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("binaries")
                    .join(format!(
                        "musicdecode-ffmpeg-{}{suffix}",
                        env!("MUSICDECODE_TARGET")
                    ))
            });
        assert!(
            ffmpeg.is_file(),
            "Build the audio sidecar before running integration tests"
        );
        let d = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let original = d.path().join("tone.mp3");
        ffmpeg_run(
            &ffmpeg,
            &[
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "sine=frequency=440:duration=1".into(),
                "-codec:a".into(),
                "libmp3lame".into(),
                display_path(&original),
            ],
            &cancel,
            d.path(),
        )
        .unwrap();
        let wrapped_mp3 = d.path().join("same-audio.ncm");
        std::fs::write(
            &wrapped_mp3,
            fixture_with_cover(
                &std::fs::read(&original).unwrap(),
                8192,
                true,
                include_bytes!("../icons/32x32.png"),
            ),
        )
        .unwrap();
        let out = convert(
            &wrapped_mp3,
            Some(d.path()),
            320,
            &ffmpeg,
            &cancel,
            |_, _| {},
        )
        .unwrap();
        fn payload(bytes: &[u8]) -> &[u8] {
            if bytes.starts_with(b"ID3") {
                let n = ((bytes[6] as usize) << 21)
                    | ((bytes[7] as usize) << 14)
                    | ((bytes[8] as usize) << 7)
                    | bytes[9] as usize;
                &bytes[10 + n..]
            } else {
                bytes
            }
        }
        assert_eq!(
            Sha256::digest(payload(&std::fs::read(&original).unwrap())),
            Sha256::digest(payload(&std::fs::read(&out).unwrap()))
        );
        assert_eq!(Tag::read_from_path(&out).unwrap().title(), Some("fixture"));
        assert_eq!(Tag::read_from_path(&out).unwrap().pictures().count(), 1);
        let second = convert(
            &wrapped_mp3,
            Some(d.path()),
            320,
            &ffmpeg,
            &cancel,
            |_, _| {},
        )
        .unwrap();
        assert_ne!(out, second);
        let inflight = d.path().join("cancelled");
        assert!(convert(
            &wrapped_mp3,
            Some(&inflight),
            320,
            &ffmpeg,
            &cancel,
            |pct, _| {
                if pct > 0 {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
        )
        .is_err());
        assert_eq!(std::fs::read_dir(inflight).unwrap().count(), 0);
        cancel.store(false, Ordering::SeqCst);
        let flac = d.path().join("fixture.flac");
        ffmpeg_run(
            &ffmpeg,
            &[
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "sine=frequency=440:duration=1".into(),
                "-codec:a".into(),
                "flac".into(),
                display_path(&flac),
            ],
            &cancel,
            d.path(),
        )
        .unwrap();
        let wrapped = d.path().join("中文 测试.ncm");
        std::fs::write(
            &wrapped,
            fixture(&std::fs::read(flac).unwrap(), 1024, false),
        )
        .unwrap();
        let converted =
            convert(&wrapped, Some(d.path()), 192, &ffmpeg, &cancel, |_, _| {}).unwrap();
        assert!(std::fs::read(converted).unwrap().starts_with(b"ID3"));
        for (extension, codec) in [("m4a", "aac"), ("wav", "pcm_s16le")] {
            let encoded = d.path().join(format!("fixture.{extension}"));
            ffmpeg_run(
                &ffmpeg,
                &[
                    "-f".into(),
                    "lavfi".into(),
                    "-i".into(),
                    "sine=frequency=440:duration=1".into(),
                    "-codec:a".into(),
                    codec.into(),
                    display_path(&encoded),
                ],
                &cancel,
                d.path(),
            )
            .unwrap();
            let wrapped = d.path().join(format!("fixture-{extension}.ncm"));
            std::fs::write(
                &wrapped,
                fixture(&std::fs::read(encoded).unwrap(), 0, false),
            )
            .unwrap();
            assert!(convert(&wrapped, Some(d.path()), 256, &ffmpeg, &cancel, |_, _| {}).is_ok());
        }
        let bad = d.path().join("bad-audio.ncm");
        std::fs::write(&bad, fixture(b"ID3\x03\0\0\0\0\0\0invalid-audio", 0, false)).unwrap();
        assert!(convert(&bad, Some(d.path()), 320, &ffmpeg, &cancel, |_, _| {}).is_err());
        assert!(!d.path().join("bad-audio.mp3").exists());
        assert!(!std::fs::read_dir(d.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().starts_with(".musicdecode-")));
    }
}
