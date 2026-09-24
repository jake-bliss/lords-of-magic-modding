# Notices

**This mod** -- the setup script and tools -- is free, non-commercial fan work. Lords of Magic is (c)
Sierra / Impressions Games; no part of it is distributed here.

**ddraw.dll** is a modified build of [cnc-ddraw](https://github.com/FunkyFr3sh/cnc-ddraw) 7.1.0.0,
MIT licence, (c) 2022 github.com/FunkyFr3sh. Full text in `LICENSE-cnc-ddraw.txt`. The
modification adds the portrait overlay (`src/lomhd.c`, `src/lomhd_match.c`) and serves the HD
terrain from `lomhd_terrain`.

**HD terrain (`--terrain`)** patches your own `lomse.exe` on your machine, from the edit lists in
`exe_patches/`. No part of `lomse.exe` is distributed here: the lists hold only the few bytes each
edit expects to find and what it puts there. The terrain art is made on your machine from your own
`pic.mpq`, like the pictures.

**tools/mpq_read.py** contains a Python port of `blast.c` by Mark Adler (zlib `contrib/blast`),
(c) 2003, 2012, 2013 Mark Adler (blast.c 1.3), zlib licence:

> This software is provided 'as-is', without any express or implied warranty. In no event will the
> author be held liable for any damages arising from the use of this software. Permission is
> granted to anyone to use this software for any purpose, including commercial applications, and to
> alter it and redistribute it freely, subject to the following restrictions: 1. The origin of this
> software must not be misrepresented; you must not claim that you wrote the original software. If
> you use this software in a product, an acknowledgment in the product documentation would be
> appreciated but is not required. 2. Altered source versions must be plainly marked as such, and
> must not be misrepresented as being the original software. 3. This notice may not be removed or
> altered from any source distribution.

**Downloaded at setup, not included:**

- [Real-ESRGAN ncnn Vulkan](https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan) v0.2.5.0, MIT
  licence, (c) 2021 Xintao Wang, with the `realesr-animevideov3` models bundled in that release
  (from [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN), BSD 3-Clause licence, (c) 2021
  Xintao Wang). The shipped HD terrain picks use only these anime models (MIT / BSD 3-Clause), not 4x-UltraSharp.
- [4x-UltraSharp](https://openmodeldb.info/models/4x-UltraSharp) by Kim2091, **CC BY-NC-SA 4.0**
  (non-commercial, share-alike), in the ncnn conversion distributed by
  [Upscayl](https://github.com/upscayl/upscayl). The portraits made with it on your machine are for
  your own non-commercial use.
