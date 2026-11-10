# Workshop: framed networking

TCP delivers an ordered byte stream. It does not preserve application writes.
UDP delivers datagrams. It does not promise that they arrive, arrive once, or
arrive in order. This workshop uses the checked loopback fixtures
[`socket_echo`](../../../tests/golden/run/socket_echo/src/main.sv),
[`udp_datagrams`](../../../tests/golden/run/udp_datagrams/src/main.sv), and
Release Radar's
[`fixture.sv`](../../../examples/release_radar/src/release_radar/fixture.sv).

Run the focused fixtures through the golden suite:

```sh
cargo xtask test host
```

## Scope ownership first

`withTcpListener anyPort callback` binds loopback and passes both a listener
and the selected `Port`. `withAcceptedTcpConnection` gives each accepted
connection its own nested scope. `withTcpConnection` does the same for a
client. Rank-2 `scope` parameters prevent all three live resources and their
derived streams from escaping callbacks.

The socket-echo ownership tree is:

```text
withTcpListener
├── withThread server
│   └── withAcceptedTcpConnection
└── withTcpConnection client
```

Leaving any owner runs cleanup on success, typed error, defect, or interruption.

## Do not confuse a read with a frame

`socket_echo` is deliberately tiny: its server reads up to 1024 bytes once and
echoes that chunk. That is valid only for its controlled short fixture; it is
not a general echo protocol guarantee.

Release Radar's local service demonstrates delimiter framing. It accumulates
chunks until it finds `CRLF CRLF`, the HTTP header terminator. A production
delimiter protocol must also enforce a maximum accumulated size. Otherwise a
peer that never sends the delimiter can grow memory without bound.

A length-prefixed protocol needs:

1. a fixed-size prefix;
2. a loop that reads exactly the missing prefix bytes;
3. safe integer decoding;
4. validation against a maximum frame size before allocating;
5. another exact-read loop for the payload;
6. a decision about EOF in the middle of either phase.

One `sendTcpBytes` may be observed through several receives, and several sends
may be coalesced into one receive. Only the application framing rules identify
messages.

## Demand-driven TCP streams

`tcpBytes connection 4` in `socket_echo` yields at most four bytes per pull.
The fixture folds chunks with `bytesAppend` until EOF. The chunk bound controls
one host read, not total response size. Folding into one `Bytes` value still
retains the complete response.

For large data, fold into a small summary or write each chunk to a scoped file.
If total size is a trust boundary, count bytes and fail at an explicit quota.

## UDP has different rules

`udp_datagrams` verifies:

- resolver results are typed `IpAddress` values;
- the received `Datagram` retains its source `Endpoint`;
- an empty datagram is data, not EOF;
- a payload larger than the receive bound is consumed and reported as error,
  not silently truncated;
- a subsequent datagram can still be received;
- a blocked receive wakes when its owning thread is cancelled.

`receiveDatagram socket maximum` accepts bounds from 1 through 65,535.
`datagrams` is a scoped stream with one receive demand per pull. UDP has no EOF
signal, so scope exit, a protocol message, a typed failure, or cancellation
must terminate a receive loop.

## Choose the transport from semantics

Use TCP when the application needs reliable ordered bytes and can define
framing. Use UDP when datagram boundaries and latency matter more than reliable
delivery, and the application is prepared to define loss, duplication,
ordering, fragmentation, and retry policy. Use `std.http` for web APIs;
manually reproducing HTTP over TCP discards URL, TLS, redirect, framing, and
limit policy already implemented by the client.

## Hermetic testing

Bind loopback with `anyPort`, pass the selected typed port to the client, own
the server with `withThread`, accept a finite request count, and avoid fixed
ports. Test framing by deliberately splitting input at multiple boundaries.
Test cancellation while a receive is blocked. Never require public DNS or a
public service for protocol correctness.

## Experiments

1. Turn `socket_echo` into newline-delimited frames with a maximum of 4096
   bytes.
2. Feed the delimiter one byte at a time and then in one chunk; require the same
   decoded frame.
3. Add a two-byte length prefix and reject a frame above policy before reading
   its body.
4. Send two UDP datagrams and record observed order without asserting that the
   network guarantees it.

## Review checklist

- What creates message boundaries?
- What bounds a chunk, a frame, and the total session?
- How is premature EOF represented?
- Who owns every listener, connection, socket, and stream?
- What wakes blocked IO during cancellation?
- Can the test run entirely on loopback?
