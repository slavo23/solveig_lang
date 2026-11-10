# Workshop: a bounded-memory ray tracer

[`examples/ray_tracer`](../../../examples/ray_tracer/) renders a deterministic
512×512 ASCII PPM scene with spheres, an area light, soft shadows, highlights,
and two reflection bounces:

```sh
cargo run -- --project-root examples/ray_tracer
```

The repository integration suite renders that full resolution with the default
heap and checks the output structure. The example README records the rendering
contract; this workshop explains how the program reaches it.

## Give geometry domain types

```solveig
data Vec3 = Vec3 Float Float Float
data Ray = Ray Vec3 Vec3
data Material = Material Vec3 Float Float
data Sphere = Sphere Vec3 Float Material
data Hit = Hit Vec3 Vec3 Material
```

The same `Vec3` representation serves points, directions, and colors. That is
compact but not dimensionally perfect: the typechecker cannot stop adding two
points. A larger renderer could introduce distinct wrappers while reusing pure
vector functions.

`lengthVec = sqrt . dotSelf` and `negateVec = scaleVec (-1.0)` use
point-free style where it exposes the algebra. Intersection and shading keep
named values because the equations are easier to audit that way.

## Generate one camera ray per pixel

`cameraFor width height` computes an aspect-correct viewport.
`cameraRay camera u v` returns a ray from the camera origin through normalized
pixel coordinates. `renderPixel` converts integer coordinates with
`std.math.fromInt`; no implicit numeric coercion is involved.

The renderer samples one ray at each pixel, so it has sharp geometric edges.
Anti-aliasing would trace several subpixel rays and average them. Use a fixed
`StdGen` seed if sample positions are randomized so images and bug reports are
reproducible.

## Solve ray–sphere intersection

`intersectionDistance` substitutes the ray equation into the sphere equation
and solves the quadratic. A negative discriminant means no hit. It checks the
near root first, then the far root, and accepts only the caller's
`minimum..maximum` interval.

`nearestHitFrom` carries the closest distance found so far. Every later sphere
uses that distance as its maximum, so a farther hit cannot replace a nearer
one. Shadow tests use `occluded`, which stops at the first blocker and avoids
constructing a full `Hit`.

The minimum `0.002` prevents a new ray starting exactly on a surface from
immediately hitting that same surface due to floating-point error. It is a
scene-scale bias, not a universal constant.

## Produce soft shadows and highlights

The area light has four fixed sample offsets. For each sample,
`lightSample` computes:

- direction and squared distance;
- a Lambertian diffuse factor;
- bounded occlusion only up to the light;
- distance attenuation;
- a Blinn–Phong-style `pow32` highlight.

`areaLighting` averages the four samples. Some sample rays can be blocked while
others reach the light, producing a penumbra. More samples improve the estimate
but linearly add shadow rays.

## Bound recursive reflection

`traceRay world 2` supplies a bounce budget. At a hit, `shadeHit` always
computes local color. It traces another ray only when the budget and material
reflectivity are positive, then blends local and reflected colors.

The direction uses `d - 2 * dot d n * n`; the origin is offset above the
surface. The fixed budget makes termination and worst-case ray growth explicit.
It is not a physically based energy-conservation model, but it is a coherent,
documented rendering choice.

## Why the 512×512 version fits

An earlier whole-image builder retained millions of builder nodes and small
strings until the final file write. The current pipeline retains one row:

```text
pixel colors
  └─ TextBuilder for one row
       └─ buildText
            └─ hPutStr to 64 KiB buffered file
                 └─ row becomes unreachable
```

`TextBuilder` gives constant-time concatenation inside a row.
`renderRowsTo` writes the row before recursing. The live output representation
therefore scales with width, not width × height. `withFile` scopes the handle,
and block buffering avoids one host write per small text fragment.

This is the relevant heap lesson: garbage collection can reclaim only objects
that are no longer reachable. A semantically correct program that retains the
whole output graph still needs memory proportional to that graph.

## Why green-thread concurrency is not the optimization

Rendering is CPU-bound pure work. The current VM executes Solveig bytecode on
one scheduler OS thread, so multiple green threads would interleave pixels
without running them on multiple cores. Concurrency helps the repository's HTTP
and file-waiting applications; it does not provide CPU parallelism here.

Useful renderer optimizations are algorithmic and representational: fewer
temporary values, early occlusion exit, spatial indexing for many objects,
binary PPM, and bounded row/tile retention. External worker processes would be
a different parallel architecture with serialization and merge costs.

## Experiments

1. Render 64×64 while changing one material at a time.
2. Increase the area-light radius and explain the penumbra change.
3. Set the bounce budget to zero, one, and two.
4. Add seeded subpixel samples and keep exact reproducibility.
5. Design binary `P6` output using `Bytes`, retaining one row.
6. Add a plane or triangle and keep `nearestHit`'s closest-distance invariant.

## Review checklist

- Are points, directions, normals, and colors normalized or clamped at the
  correct boundary?
- Do intersection ranges prevent self-hits and hits beyond a light?
- Is recursion bounded?
- Is sampling deterministic when tests require it?
- Does output retention scale with row/tile size rather than image size?
- Is a proposed concurrency change actually parallel on the current VM?
