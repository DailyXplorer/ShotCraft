# Text recognition models

The two files here are the Ocrs models. ShotCraft uses them for text and QR recognition on Linux,
where they are compiled into the app (`src/ocr.rs`). Windows uses its own built-in OCR and macOS
uses Vision, so their builds don't contain these files.

## Attribution

Ocrs models (https://github.com/robertknight/ocrs-models), trained on the HierText dataset by
Google Research. Licensed under CC BY-SA 4.0.

## Licence

Creative Commons Attribution-ShareAlike 4.0 International (CC BY-SA 4.0): the full text is in
[`LICENSE.txt`](LICENSE.txt), also at https://creativecommons.org/licenses/by-sa/4.0/. The models
inherit it from the HierText dataset (https://github.com/google-research-datasets/hiertext), as the
ocrs-models licence states (https://github.com/robertknight/ocrs-models/blob/main/LICENSE.md).

This licence covers these two files only. The rest of ShotCraft keeps its own licence (`LICENSE` at
the top of the repository).

## The files

They are unchanged, byte for byte, as their authors publish them (the files `ocrs-cli` downloads):

| File | Source | SHA-256 |
|---|---|---|
| `text-detection.onnx` | https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.onnx | `a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe` |
| `text-recognition.onnx` | https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.onnx | `86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5` |

ShotCraft never downloads them: they ship with the source, and the app makes no network connection
to load them.
