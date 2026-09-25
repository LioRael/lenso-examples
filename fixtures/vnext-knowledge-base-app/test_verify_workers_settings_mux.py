import asyncio
import struct
import unittest

from verify_workers_settings_mux import DATA, HEADER, MAX_FRAME, read_frame


class RelayFrameTests(unittest.IsolatedAsyncioTestCase):
    async def test_reads_complete_framed_payload(self):
        reader = asyncio.StreamReader()
        reader.feed_data(HEADER.pack(DATA, 7, 4) + b"test")
        reader.feed_eof()
        self.assertEqual(await read_frame(reader), (DATA, 7, b"test"))

    async def test_rejects_oversize_frame_before_payload(self):
        reader = asyncio.StreamReader()
        reader.feed_data(struct.pack(">BII", DATA, 7, MAX_FRAME + 1))
        reader.feed_eof()
        with self.assertRaisesRegex(ValueError, "exceeds bound"):
            await read_frame(reader)

    async def test_truncated_frame_fails_closed(self):
        reader = asyncio.StreamReader()
        reader.feed_data(HEADER.pack(DATA, 7, 4) + b"ab")
        reader.feed_eof()
        with self.assertRaises(asyncio.IncompleteReadError):
            await read_frame(reader)


if __name__ == "__main__":
    unittest.main()
