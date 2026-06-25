import argparse
import asyncio
import pathlib
import sys

import flatbuffers

ROOT = pathlib.Path(__file__).resolve().parent
GENERATED = ROOT / "generated"
if str(GENERATED) not in sys.path:
    sys.path.insert(0, str(GENERATED))

from tensor_rag.parser import ParseRequest, ParseResponse
from tensor_rag.parser import ParserBackend


async def read_frame(reader: asyncio.StreamReader) -> bytes:
    raw_len = await reader.readexactly(4)
    frame_len = int.from_bytes(raw_len, "little")
    return await reader.readexactly(frame_len)


async def write_frame(writer: asyncio.StreamWriter, payload: bytes) -> None:
    writer.write(len(payload).to_bytes(4, "little"))
    writer.write(payload)
    await writer.drain()


def build_request() -> bytes:
    builder = flatbuffers.Builder(1024)
    job_id = builder.CreateString("smoke-job")
    file_path = builder.CreateString("F:/docs/sample.docx")
    filename = builder.CreateString("sample.docx")
    mime_type = builder.CreateString(
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    )

    ParseRequest.StartRequestedBackendsVector(builder, 1)
    builder.PrependInt8(ParserBackend.ParserBackend.DocxNative)
    requested_backends = builder.EndVector()

    ParseRequest.StartParserHintsVector(builder, 0)
    parser_hints = builder.EndVector()

    ParseRequest.Start(builder)
    ParseRequest.AddJobId(builder, job_id)
    ParseRequest.AddFilePath(builder, file_path)
    ParseRequest.AddFilename(builder, filename)
    ParseRequest.AddMimeType(builder, mime_type)
    ParseRequest.AddParserHints(builder, parser_hints)
    ParseRequest.AddRequestedBackends(builder, requested_backends)
    request = ParseRequest.End(builder)
    builder.Finish(request)
    return bytes(builder.Output())


async def main() -> None:
    parser = argparse.ArgumentParser(description="Smoke client for parser sidecar")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=47777)
    args = parser.parse_args()

    reader, writer = await asyncio.open_connection(args.host, args.port)
    await write_frame(writer, build_request())
    payload = await read_frame(reader)
    response = ParseResponse.ParseResponse.GetRootAs(payload, 0)
    print(
        {
            "job_id": response.JobId().decode("utf-8"),
            "ok": response.Ok(),
            "error_code": response.ErrorCode().decode("utf-8"),
            "error_message": response.ErrorMessage().decode("utf-8"),
        }
    )
    writer.close()
    await writer.wait_closed()


if __name__ == "__main__":
    asyncio.run(main())
