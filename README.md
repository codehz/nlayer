# native-layer

One TSX application, run on web, iOS and Android. Mechanisms are extracted
from [nsh](https://github.com/longbridge/nsh); existing nsh apps are not
compatible.

v1 is the `nlayer` CLI (`init` / `build` / `run --target web|ios|android`)
plus a headless TSX test runner. The public surface is `View`, `Text`,
`Image`, `Scroll` and single-line `Input`, with the nsh reactive graph and
Taffy layout. See `NOTICE` for the Apache-2.0 provenance of copied code.

Licensed under the [Apache License, Version 2.0](LICENSE-APACHE).
