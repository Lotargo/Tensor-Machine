import argparse
import asyncio
import pathlib
import sys

import flatbuffers

ROOT = pathlib.Path(__file__).resolve().parent
GENERATED = ROOT / "generated"
if str(GENERATED) not in sys.path:
    sys.path.insert(0, str(GENERATED))

from tensor_rag.parser.ParseRequest import ParseRequest
from tensor_rag.parser import ParseResponse


MAX_FRAME_BYTES = 128 * 1024 * 1024


async def read_frame(reader: asyncio.StreamReader) -> bytes:
    raw_len = await reader.readexactly(4)
    frame_len = int.from_bytes(raw_len, "little")
    if frame_len > MAX_FRAME_BYTES:
        raise ValueError(f"frame too large: {frame_len}")
    return await reader.readexactly(frame_len)


async def write_frame(writer: asyncio.StreamWriter, payload: bytes) -> None:
    if len(payload) > MAX_FRAME_BYTES:
        raise ValueError(f"frame too large: {len(payload)}")
    writer.write(len(payload).to_bytes(4, "little"))
    writer.write(payload)
    await writer.drain()


def text(value):
    if value is None:
        return ""
    return value.decode("utf-8") if isinstance(value, bytes) else str(value)


def build_error_response(job_id: str, code: str, message: str) -> bytes:
    builder = flatbuffers.Builder(1024)
    job_id_offset = builder.CreateString(job_id)
    code_offset = builder.CreateString(code)
    message_offset = builder.CreateString(message)

    ParseResponse.Start(builder)
    ParseResponse.AddJobId(builder, job_id_offset)
    ParseResponse.AddOk(builder, False)
    ParseResponse.AddErrorCode(builder, code_offset)
    ParseResponse.AddErrorMessage(builder, message_offset)
    response = ParseResponse.End(builder)
    builder.Finish(response)
    return bytes(builder.Output())


async def handle_client(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    try:
        while True:
            payload = await read_frame(reader)
            request = ParseRequest.GetRootAs(payload, 0)
            job_id = text(request.JobId())
            filename = text(request.Filename())
            response = build_error_response(
                job_id,
                "NOT_IMPLEMENTED",
                f"No parser backend is implemented yet for {filename}",
            )
            await write_frame(writer, response)
    except asyncio.IncompleteReadError:
        pass
    finally:
        writer.close()
        await writer.wait_closed()


async def main() -> None:
    parser = argparse.ArgumentParser(description="APL-RAG parser sidecar")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=47777)
    args = parser.parse_args()

    server = await asyncio.start_server(handle_client, args.host, args.port)
    addrs = ", ".join(str(sock.getsockname()) for sock in server.sockets or [])
    print(f"parser sidecar listening on {addrs}", flush=True)
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    asyncio.run(main())
