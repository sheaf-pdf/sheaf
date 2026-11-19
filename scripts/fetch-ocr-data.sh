#!/usr/bin/env bash
# Download the OCR engines' data into vendor/ocr/ so sheaf-ocr can find it:
#   vendor/ocr/tessdata/eng.traineddata   Tesseract English (tessdata_fast)
#   vendor/ocr/tessdata/osd.traineddata   Tesseract orientation detection
#   vendor/ocr/models/*.onnx, *.txt       PaddleOCR PP-OCRv5 models for oar-ocr
#   vendor/ocr/lib/libonnxruntime.so      Microsoft's ONNX Runtime (onnxruntime.dll
#                                         on Windows), loaded at run time
# Usage: scripts/fetch-ocr-data.sh [--multilingual] [--uvdoc]
#   --uvdoc also fetches the UVDoc page-unwarping model (about 31 MB) that
#   scan cleanup's "flatten curled pages" uses.
# The app looks in $SHEAF_OCR_DATA_DIR, ocr/ beside the binary, then vendor/ocr.
set -euo pipefail
cd "$(dirname "$0")/.."

TESSDATA_REF="${TESSDATA_REF:-main}"
OAR_MODELS_TAG="${OAR_MODELS_TAG:-v0.3.0}"
# Same ONNX Runtime version as the `ort` crate's own prebuilt binaries.
ORT_VERSION="${ORT_VERSION:-1.28.0}"
multilingual=0
uvdoc=0
for arg in "$@"; do
  case "$arg" in
    --multilingual) multilingual=1 ;;
    --uvdoc) uvdoc=1 ;;
    *) echo "unknown option $arg" >&2; exit 1 ;;
  esac
done

dest="vendor/ocr"
mkdir -p "$dest/tessdata" "$dest/models" "$dest/lib"

fetch() { # url dest
  if [[ -s "$2" ]]; then
    echo "have $2"
  else
    echo "fetching $1"
    curl -fsSL "$1" -o "$2.part" && mv "$2.part" "$2"
  fi
}

tess="https://github.com/tesseract-ocr/tessdata_fast/raw/${TESSDATA_REF}"
fetch "$tess/eng.traineddata" "$dest/tessdata/eng.traineddata"
# Orientation detection, for turning scanned and photographed pages upright.
fetch "$tess/osd.traineddata" "$dest/tessdata/osd.traineddata"
fetch "$tess/LICENSE" "$dest/tessdata/LICENSE"

oar="https://github.com/GreatV/oar-ocr/releases/download/${OAR_MODELS_TAG}"
models=(
  pp-ocrv5_mobile_det.onnx
  en_pp-ocrv5_mobile_rec.onnx
  ppocrv5_en_dict.txt
  pp-lcnet_x0_25_textline_ori.onnx
  pp-lcnet_x1_0_doc_ori.onnx
)
if [[ $multilingual == 1 ]]; then
  models+=(pp-ocrv5_mobile_rec.onnx ppocrv5_dict.txt)
fi
if [[ $uvdoc == 1 ]]; then
  models+=(uvdoc.onnx)
fi
for m in "${models[@]}"; do
  fetch "$oar/$m" "$dest/models/$m"
done
fetch "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/LICENSE" "$dest/models/LICENSE"

# ONNX Runtime: Microsoft's generic x86-64 build (runtime CPU dispatch, so
# no AVX2 requirement), unlike the x86-64-v3 static binaries `ort` downloads.
# Fetched from Microsoft's official `Microsoft.ML.OnnxRuntime` NuGet package
# (microsoft/onnxruntime on GitHub went offline in Nov 2026; the NuGet `.nupkg`
# distributes the identical Microsoft-built shared libraries).
ORT_NUPKG_SHA256_1_28_0="769d1d3ea8ab6cd69f737c9dd4d4462aa4ad0ccfa106eaf506efc40d7bead5db"
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  ort_rid="linux-x64";  ort_lib="libonnxruntime.so" ;;
  Linux-aarch64) ort_rid="linux-arm64"; ort_lib="libonnxruntime.so" ;;
  Darwin-arm64)  ort_rid="osx-arm64";  ort_lib="libonnxruntime.dylib" ;;
  MINGW64*|MSYS*|CYGWIN*) ort_rid="win-x64"; ort_lib="onnxruntime.dll" ;;
  *) echo "unknown host for ONNX Runtime; set it up by hand" >&2; ort_rid="" ;;
esac
if [[ -n "$ort_rid" && ! -s "$dest/lib/$ort_lib" ]]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  ort_nupkg="https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime/${ORT_VERSION}/microsoft.ml.onnxruntime.${ORT_VERSION}.nupkg"
  fetch "$ort_nupkg" "$tmp/onnxruntime.zip"
  if [[ "$ORT_VERSION" == "1.28.0" ]]; then
    if command -v sha256sum >/dev/null 2>&1; then
      got="$(sha256sum "$tmp/onnxruntime.zip" | awk '{print $1}')"
    else
      got="$(shasum -a 256 "$tmp/onnxruntime.zip" | awk '{print $1}')"
    fi
    [[ "$got" == "$ORT_NUPKG_SHA256_1_28_0" ]] || {
      echo "onnxruntime.zip SHA-256 $got != $ORT_NUPKG_SHA256_1_28_0" >&2
      exit 1
    }
  fi
  mkdir -p "$tmp/x"
  if command -v unzip >/dev/null 2>&1; then
    unzip -q "$tmp/onnxruntime.zip" "runtimes/$ort_rid/native/$ort_lib" "LICENSE" -d "$tmp/x"
  else
    # Git Bash has no unzip unless added; PowerShell Expand-Archive extracts .zip.
    powershell.exe -NoProfile -Command \
      "Expand-Archive -Path '$(cygpath -w "$tmp/onnxruntime.zip")' -DestinationPath '$(cygpath -w "$tmp/x")' -Force"
  fi
  found="$tmp/x/runtimes/$ort_rid/native/$ort_lib"
  [[ -s "$found" ]] || { echo "no $ort_lib in Microsoft.ML.OnnxRuntime ${ORT_VERSION} ($ort_rid)" >&2; exit 1; }
  cp "$found" "$dest/lib/$ort_lib"
  [[ -s "$tmp/x/LICENSE" ]] && tr -d '\r' < "$tmp/x/LICENSE" > "$dest/lib/LICENSE"
fi

echo "installed into $dest:"; du -sh "$dest"/*; ls -l "$dest/models" "$dest/tessdata" "$dest/lib"
