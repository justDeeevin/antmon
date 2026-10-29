Antmon has a few c library dependencies on Linux. You will need these available at compile-time, but they get statically linked.
- libx11 ^1.4.99.1
- libxi ^1.7
- libxtst ^1.2
- libinput ^1.19.1
- libxkbcommon ^1.0[^1]

[^1]: I can't tell for sure what the minimum version is here. The xkbcommon crate doesn't use pkg-config for its c dependency, so there's nothing within its source indicating a specific required version; it just links against anything named xkbcommon.
