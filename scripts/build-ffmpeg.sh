#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${MUSICDECODE_TARGET:-$(rustc --print host-tuple)}"
case "$TARGET" in
  x86_64-pc-windows-msvc) SUFFIX=.exe ;;
  aarch64-apple-darwin|x86_64-unknown-linux-gnu) SUFFIX= ;;
  *) echo "Unsupported target: $TARGET" >&2; exit 1 ;;
esac
WORK="$ROOT/work/audio-$TARGET"
PREFIX="$WORK/prefix"
mkdir -p "$WORK/ffmpeg" "$WORK/lame" "$PREFIX" "$ROOT/src-tauri/binaries" "$ROOT/third-party/generated"
tar -xzf "$ROOT/work/sources/lame.tar.gz" --strip-components=1 -C "$WORK/lame"
tar -xzf "$ROOT/work/sources/ffmpeg.tar.gz" --strip-components=1 -C "$WORK/ffmpeg"
JOBS="${NUMBER_OF_PROCESSORS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 3)}"
export CFLAGS="-O2"
if [[ "$TARGET" == aarch64-apple-darwin ]]; then
  export MACOSX_DEPLOYMENT_TARGET=12.0
  export CFLAGS="-O2 -mmacosx-version-min=12.0 -Wno-error=implicit-function-declaration"
fi
cd "$WORK/lame"
./configure --prefix="$PREFIX" --disable-shared --enable-static --disable-frontend --disable-decoder --with-pic
make -j"$JOBS"
make install

cd "$WORK/ffmpeg"
EXTRA=(--extra-version=MusicDecode)
if [[ "$TARGET" == x86_64-pc-windows-msvc ]]; then
  EXTRA+=(--target-os=mingw32 --arch=x86_64 --cc=gcc --cxx=g++ "--extra-ldflags=-L$PREFIX/lib -static -static-libgcc")
fi
CONFIG=(
  --prefix="$PREFIX" --disable-everything --disable-autodetect --disable-network
  --disable-doc --disable-debug --disable-shared --enable-static --disable-x86asm
  --disable-gpl --disable-nonfree --disable-ffplay --disable-ffprobe --enable-ffmpeg
  --enable-libmp3lame --enable-avcodec --enable-avformat --enable-avfilter --enable-avdevice
  --enable-swresample --enable-protocol=file,pipe
  --enable-demuxer=mp3,flac,mov,wav,ogg --enable-indev=lavfi
  --enable-parser=mpegaudio,flac,aac,vorbis,opus,png,mjpeg
  --enable-decoder=mp3,mp3float,mp3adu,mp3adufloat,flac,aac,vorbis,opus,pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le,pcm_f64le,mjpeg
  --enable-encoder=libmp3lame,flac,pcm_s16le,aac
  --enable-muxer=mp3,flac,null,wav,ipod
  --enable-filter=aresample,anull,sine,aformat,abuffer,abuffersink
  --extra-cflags="-I$PREFIX/include" --extra-ldflags="-L$PREFIX/lib"
  --pkg-config-flags=--static
)
./configure "${CONFIG[@]}" "${EXTRA[@]}"
make -j"$JOBS"
BIN="$ROOT/src-tauri/binaries/musicdecode-ffmpeg-$TARGET$SUFFIX"
cp "ffmpeg$SUFFIX" "$BIN"
chmod +x "$BIN"
if [[ "$TARGET" == aarch64-apple-darwin ]]; then codesign --force --sign - "$BIN"; fi
cp COPYING.LGPLv2.1 "$ROOT/third-party/generated/FFmpeg-LICENSE.txt"
cp "$WORK/lame/COPYING" "$ROOT/third-party/generated/LAME-LICENSE.txt"
{
  echo "MusicDecode audio component / $TARGET"
  echo "FFmpeg 9.0.2 and LAME 3.100, compiled from the SHA-256-pinned sources in third-party/sources.json."
  echo "The complete build recipe is scripts/build-ffmpeg.sh."
  echo "GPL, nonfree components and networking are disabled. FFmpeg and LAME are statically linked."
  printf 'FFmpeg configuration: '; printf '%q ' "${CONFIG[@]}" "${EXTRA[@]}"; echo
  "$BIN" -version
  "$BIN" -L
} > "$ROOT/third-party/generated/BUILD.txt" 2>&1
"$BIN" -hide_banner -encoders 2>&1 | grep libmp3lame
"$BIN" -hide_banner -decoders 2>&1 | grep flac
echo "Audio sidecar ready: $BIN"
