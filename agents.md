本项目面向Mac和win开发，使用zig交叉编译。

当实现Rust代码修改时，依次执行：

cargo fmt --all

cargo check --workspace --all-targets

cargo clippy --workspace --all-targets -- -D warnings
