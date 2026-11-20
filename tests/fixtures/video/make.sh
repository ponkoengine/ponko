#!/bin/sh
# Makes the video test files from generated pictures and tones (no footage):
# 64 x 64, 10 frames a second, 2 seconds, a keyframe every 5 frames.
# Each frame has red, green and blue quarters and a gray quarter whose level
# says which frame it is (20 + 10 * frame). The sound is a 440 Hz tone for
# the first second and 880 Hz for the second.
# Needs Python 3 and an ffmpeg built with SVT-AV1, libvpx and libopus.
set -eu
cd "$(dirname "$0")"
frames() {
    python3 -c '
import sys
W, H, N = 64, 64, 20
for k in range(N):
    level = 20 + k * 10
    top = bytes([255, 0, 0]) * (W // 2) + bytes([0, 255, 0]) * (W // 2)
    bottom = bytes([0, 0, 255]) * (W // 2) + bytes([level] * 3) * (W // 2)
    sys.stdout.buffer.write(top * (H // 2) + bottom * (H // 2))
'
}
tone="aevalsrc=0.25*sin(2*PI*if(lt(t\,1)\,440\,880)*t):s=48000:d=2"
color="-pix_fmt yuv420p -color_primaries bt709 -color_trc bt709 -colorspace bt709 -color_range tv"
av1="-c:v libsvtav1 -preset 8 -crf 20 -g 5 $color"
in="-f rawvideo -pix_fmt rgb24 -s 64x64 -r 10 -i -"
q="-hide_banner -loglevel error -y -bitexact"
# AV1 picture, Vorbis sound: what the engine plays.
frames | ffmpeg $q $in -f lavfi -i "$tone" -ac 2 $av1 -c:a vorbis -strict -2 -q:a 2 -shortest clip.webm
# AV1 picture, no sound.
frames | ffmpeg $q $in $av1 -an silent.webm
# AV1 picture, Opus sound: the sound is refused.
frames | ffmpeg $q $in -f lavfi -i "$tone" -ac 2 $av1 -c:a libopus -b:a 8k -shortest opus.webm
# VP9 picture: the video is refused.
frames | ffmpeg $q $in -c:v libvpx-vp9 -g 5 $color -an vp9.webm
