#!/bin/sh
# Reads the two scripts in clips/ with macOS's built-in voices and writes
# 16 kHz mono 16-bit WAV files to the folder given (default: clips/).
# The audio is not committed; only the scripts are.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-$here/clips}
mkdir -p "$out"
say -v Tingting -o "$out/zh.aiff" -f "$here/clips/zh.txt"
say -v Samantha -o "$out/en.aiff" -f "$here/clips/en.txt"
for lang in zh en; do
  afconvert -f WAVE -d LEI16@16000 -c 1 "$out/$lang.aiff" "$out/$lang.wav"
  rm "$out/$lang.aiff"
done
ls -l "$out"/*.wav
