"""Offline one-message WebSocket fixture; no third-party Python packages."""
import base64
import hashlib
import socket
import struct

with socket.socket() as server:
    server.bind(("127.0.0.1", 0))
    server.listen(1)
    server.settimeout(15)
    print(server.getsockname()[1], flush=True)
    with server.accept()[0] as peer:
        peer.settimeout(10)
        stream = peer.makefile("rb")
        assert stream.readline().startswith(b"GET /")
        headers = {}
        while (line := stream.readline()) != b"\r\n":
            assert line
            key, value = line.decode().split(":", 1)
            headers[key.lower()] = value.strip()
        accept = base64.b64encode(hashlib.sha1(
            (headers["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()
        ).digest())
        peer.sendall(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                     b"Connection: Upgrade\r\nSec-WebSocket-Accept: " + accept + b"\r\n\r\n")
        first, second = stream.read(2)
        assert first == 0x81 and second & 0x80
        size = second & 127
        if size == 126:
            size = struct.unpack("!H", stream.read(2))[0]
        elif size == 127:
            size = struct.unpack("!Q", stream.read(8))[0]
        mask = stream.read(4)
        data = stream.read(size)
        assert bytes(c ^ mask[i % 4] for i, c in enumerate(data)) == b"ping"
        # Fragmented text with an interleaved ping exercises control-frame handling.
        peer.sendall(b"\x01\x02po\x89\x01!\x80\x02ng")
