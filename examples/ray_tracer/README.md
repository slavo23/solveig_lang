# Sphere ray tracer

This example renders a deterministic 512-by-512 scene to an ASCII PPM image
using only Solveig and its standard library. It is small enough to read from top
to bottom, but its rendering pipeline includes the same essential stages as a
larger ray tracer:

1. A pinhole camera sends one primary ray through each output pixel.
2. The quadratic ray/sphere equation finds possible intersections.
3. Only the nearest positive intersection is retained, so nearer surfaces hide
   farther ones.
4. Four fixed points on an area light produce diffuse illumination, specular
   highlights, and soft shadow boundaries.
5. Each material has an albedo, reflectivity, and specular strength. Reflection
   rays recursively trace the scene, with a fixed two-bounce limit.
6. Rays that miss receive a sky gradient. Final linear color channels are
   gamma-corrected with `sqrt` and converted to integers in `0..255`.

The fixed light samples make every run reproducible. Random samples can reduce
structured shadow noise in a larger path tracer, but then the renderer should
use an explicitly seeded `StdGen` so tests and bug reports remain repeatable.

## Run it

From the repository root:

```sh
cargo run -- --project-root examples/ray_tracer
```

The renderer replaces `examples/ray_tracer/output/scene.ppm`. Many image
viewers open PPM directly; ImageMagick can convert it with:

```sh
magick examples/ray_tracer/output/scene.ppm scene.png
```

The output directory ignores generated images, so repeated runs cannot
accumulate files.

## Why the renderer streams rows

`P3` PPM consists of a small text header followed by one red, green, and blue
integer triple per pixel. `TextBuilder` is appropriate for assembling a row:
its concatenation is constant time, and `buildText` performs one final linear
construction.

It would be a mistake to retain one builder for the entire image. At 512 by
512, millions of tiny builder nodes and channel strings would remain reachable
simultaneously even though the resulting file is only about three megabytes.
That representation can legitimately reach the VM's managed live-object
ceiling.

This renderer instead follows a bounded-memory pipeline:

```text
camera rays -> one row of colors -> one row String -> buffered file -> discard row
```

The file handle is scoped with `withFile` and uses a 64 KiB block buffer. The
header is written once; each row is built, written, and then becomes
collectable before the next row. Memory therefore scales with image width,
not total pixel count. Shadow rays also use the lightweight intersection
distance calculation and stop at the first blocker instead of constructing a
full nearest-hit record.

## Lighting and reflections

The area light is a position, radius, color, and power. Each sample computes:

- a normalized direction and distance to the sampled light point;
- a Lambertian diffuse term from the surface normal;
- distance attenuation;
- a Blinn–Phong-style specular highlight; and
- a bounded shadow ray that ignores geometry beyond the light.

Averaging the four visibility results produces a penumbra instead of a binary
hard-shadow edge. Increasing the light radius widens that penumbra; moving the
light changes its direction; increasing the sample count makes the transition
smoother but costs another shadow ray per sample.

Reflection uses the standard `d - 2 * dot d n * n` direction. The reflected ray
starts slightly above the surface to avoid immediately intersecting the sphere
that launched it. A material blends its local lighting with the recursively
traced reflection, and the two-bounce limit makes the amount of work explicit
and finite.

The renderer is sequential by design. Solveig green threads overlap network,
file, process, and timer waits, but the current VM does not execute CPU-bound
threads on multiple cores. Splitting pixels among green threads would add
coordination without reducing render time.

Useful next experiments include stratified anti-aliasing with a fixed seed,
planes and triangles, texture coordinates, a movable camera, and binary `P6`
output through `std.bytes`.
