# Upstream baseline

This directory derives from the crates.io libsqlite3-sys 0.38.1 package.
Crate checksum: `f6c19a05435c21ac299d71b6a9c13db3e3f47c520517d58990a462a1397a61db`.
Upstream Git identity: `6d3c282dc5531a57eb4e22ece3207f00c95d0fb0`, path `libsqlite3-sys`.
Bundled SQLite: 3.53.2, source identity
`d6e03d8c777cfa2d35e3b60d8ec3e0187f3e9f99d8e2ee9cac695fd6fcdf1a24`.
These describe the unmodified package, not the patched directory.

The import retains 26 upstream files, 21,623,600 bytes and 599,020 lines.
Cargo cache markers `.cargo-ok` and `.cargo-checksum.json` are excluded;
`.cargo_vcs_info.json` is retained as original provenance. The unused SQLCipher
subtree remains present, including its license; it is not an enabled build option.
Preserve the MIT license and every bundled source notice when redistributing.

The following hashes identify original bytes before the local delta. Cargo.toml.orig
and the nested Cargo.lock remain upstream reference files; the root project manifest
and root lockfile control the actual dependency selection.

| Original file | Bytes | Lines | SHA256 |
| --- | ---: | ---: | --- |
| .cargo_vcs_info.json | 108 | 6 | `d774402e1d1c2eb6f167de39e8eb210e4bfe09d1d1963e69bdf1924114b4a278` |
| .gitignore | 26 | 3 | `d89b7a44f37a816f5b0ca8297e0d17067ca5f41e45d1455b0a56d072679a00a6` |
| Cargo.lock | 7318 | 283 | `793666bc7ac1e79d904851d4cebd1c349a554dc4ba95bac89dd88404a67080d7` |
| Cargo.toml | 2380 | 120 | `b7569a8e4294b35676c3d47339dca59b50123f230c7c850fbfce251d867df252` |
| Cargo.toml.orig | 2084 | 67 | `898fedb961c33a8922d5432429eb2b7087cf6678716b736d5ec9d8762fe19145` |
| LICENSE | 1067 | 19 | `c10c1f27337546471e5f7e4e97fdd398b35b9d4e126115dcd22de8d8e65abf6f` |
| README.md | 16177 | 262 | `d4b73390386b098a1e4dd9ac477e7de8bd8fd7d4a45ee6327fc39c0f051a029d` |
| Upgrade.md | 1853 | 18 | `d151a67d4bbb1a5a300d084d210d6cd238d833a6d4bd5d958ae7469a9deccb36` |
| bindgen-bindings/bindgen_3.34.1.rs | 110115 | 3218 | `4f94d0192ec890b62773368cb026daf219f5182094952c007d28ed6d3812fd68` |
| bindgen-bindings/bindgen_3.34.1_ext.rs | 283926 | 7107 | `44ff11f09886f8c41494f65043ac2ddba4e476bb17a81fe93a8c10c7bffdf4a8` |
| build.rs | 33755 | 824 | `31fc2fb353d09faa8607c938de0fd9e963b7fbc091ce7ee750cc39e7830a5f7e` |
| sqlcipher/LICENSE | 1467 | 24 | `ea4fcb309f14a22065e1ea45362d494d320012249ed865fe9c7c0946db754131` |
| sqlcipher/bindgen_bundled_version.rs | 124545 | 3622 | `971ec006a2405a7cc443ec69500b1b6fbe45cf3f6ababf7f7815e9a3edbd6dbe` |
| sqlcipher/sqlite3.c | 9616148 | 271671 | `ea0bf0b08f688ca5d9312b2e33e7f81b3f4ae54b5016fb062ae1f2632a30a1b9` |
| sqlcipher/sqlite3.h | 673726 | 14028 | `91190701218669c31d2f56835c98882074ad051f2d38afee9f5b9c35d8406149` |
| sqlcipher/sqlite3ext.h | 38632 | 730 | `ea81fb7bd05882e0e0b92c4d60f677b205f7f1fbf085f218b12f0b5b3f0b9e48` |
| sqlite3/bindgen_bundled_version.rs | 126484 | 3677 | `5b5a37a74dc728fb0bcd041867eb577c03c95e2fe46285262c589a3b1cd4bbac` |
| sqlite3/bindgen_bundled_version_ext.rs | 317964 | 7950 | `445cc206e04a87131d4690742517ee7dcc3a430319bac847ce5e9586f98ee301` |
| sqlite3/sqlite3.c | 9507037 | 269376 | `0a409f1633283fa31a9126b11fbfd64a1991c5d30defad07e5745d4667f5e23d` |
| sqlite3/sqlite3.h | 690725 | 14347 | `9e69a1353a4288450b0d5239ede11fc7f1f4c8e5eb07491fc8317eacb5b7de7e` |
| sqlite3/sqlite3ext.h | 39175 | 739 | `ac9645e5c9ff0cf176efdd6e75cb5e98f46295d38e02db5c4d208826a39ab4be` |
| sqlite3/wasm32-wasi-vfs.c | 21443 | 690 | `b95842044da2b8777876b7d3fef8c9711e8c201fb4a4255dbac98729a456bb5d` |
| src/error.rs | 6573 | 197 | `9685678a8752bf47e5e39923bb2cf466cc312a25848d91cc206bf89eaccfb7d6` |
| src/lib.rs | 826 | 39 | `f61d8e306e11357479e20999f7b7adc5f3dbaea40cd17361853574001cf79360` |
| wrapper.h | 21 | 1 | `b78f576f7eeabf316e183d476b0f16344b6897680b4df47628d3ce38c0aa979a` |
| wrapper_ext.h | 25 | 2 | `fa7a53fea99318ee29d75e6d5fab4d7ed26e9599803268e6a24396265f2a6f3a` |

See PATCH-NOTES.md for local changes and maintenance obligations.
