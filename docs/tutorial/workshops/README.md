# Solveig workshops

The core tutorial teaches the language through two applications. These
workshops slow down around one subsystem and ask you to reason about its
invariants. They use checked repository sources; no workshop depends on an API
that exists only in prose.

1. [A bounded worker pool](01-bounded-worker-pool.md) reconstructs Log Audit's
   producer–worker–collector protocol and proves its resource bounds.
2. [A parser and interpreter](02-scry-parser-and-interpreter.md) follows Scry
   from characters to S-expressions, domain syntax, unification, search, and an
   interactive loop.
3. [Framed networking](03-framed-networking.md) explains why TCP reads are not
   messages, how UDP differs, and how to test both on loopback.
4. [Strict streams and child processes](04-streams-and-processes.md) composes
   demand-driven file/HTTP/process streams and drains subprocess pipes safely.
5. [A bounded-memory ray tracer](05-ray-tracer.md) develops the geometry,
   lighting, reflections, text building, and row-streamed PPM output.

Each workshop begins with a runnable checkpoint. Make changes in a copied
project or fixture so the repository's expected outputs remain a stable
baseline.

The workshops are not API references. Use the [documentation map](../../README.md)
for exact module guides.
