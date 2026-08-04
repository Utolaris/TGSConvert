# rlottie ![License: MIT](https://img.shields.io/badge/license-MIT-blue) [![rlottie on crates.io](https://img.shields.io/crates/v/rlottie)](https://crates.io/crates/rlottie) [![rlottie on docs.rs](https://docs.rs/rlottie/badge.svg)](https://docs.rs/rlottie) [![Source Code Repository](https://img.shields.io/badge/Code-On%20Codeberg-blue?logo=Codeberg)](https://codeberg.org/msrd0/rlottie-rs) ![Rust Version: 1.85.0](https://img.shields.io/badge/rustc-1.85.0-orange.svg)

Safe Rust bindings to rlottie.

## Example

```rust
use rlottie::{Animation, Surface};

let mut animation = Animation::from_file(path_to_lottie_json)?;
let size = animation.size();
let mut surface = Surface::new(size);
for frame in 0 .. animation.totalframe() {
	animation.render(frame, &mut surface);
	for (x, y, color) in surface.pixels() {
		println!("frame {frame} at ({x}, {y}): {color:?}");
	}
}
```
