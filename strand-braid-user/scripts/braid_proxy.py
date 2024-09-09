import requests
import json
from typing import Optional

class BraidProxy:
    DATA_PREFIX = "data: "

    def __init__(self, braid_url: str):
        self.braid_url = braid_url
        self.session = requests.session()
        self.stream = None

    def connect(self):
        """
        Connects to the Braid proxy server and initializes the event stream.
        """
        # Initial connection and check
        r = self.session.get(self.braid_url)
        if r.status_code != requests.codes.ok:
            raise ConnectionError(f"Failed to connect to Braid proxy. Status code: {r.status_code}")

        # Start Braid proxy event stream
        events_url = f"{self.braid_url}/events"
        self.stream = self.session.get(
            events_url,
            stream=True,
            headers={"Accept": "text/event-stream"},
        )

    def iter_events(self, chunk_size: Optional[int] = None, decode_unicode: bool = True, timeout: float = 1):
        """
        Iterates over events from the Braid proxy.
        """
        if not self.stream:
            raise RuntimeError("Not connected to Braid proxy. Call connect() first.")
        
        return self.stream.iter_content(chunk_size=chunk_size, decode_unicode=decode_unicode, timeout=timeout)

    def parse_chunk(self, chunk: str):
        """
        Parses a chunk of data and returns the parsed JSON object.
        """
        lines = chunk.strip().split("\n")
        if len(lines) != 2 or lines[0] != "event: braid" or not lines[1].startswith(self.DATA_PREFIX):
            raise ValueError("Invalid chunk format")
        
        buf = lines[1][len(self.DATA_PREFIX):]
        return json.loads(buf)

    def toggle_recording(self, start: bool):
        """
        Starts or stops the recording.
        """
        url = f"{self.braid_url}/callback"
        payload = {
            "DoRecordCsvTables": start
        }
        headers = {
            "Content-Type": "application/json"
        }
        
        response = self.session.post(url, json=payload, headers=headers)
        
        if response.status_code == 200:
            print(f"{'Started' if start else 'Stopped'} recording successfully")
        else:
            print(f"Failed to {'start' if start else 'stop'} recording. Status code: {response.status_code}")

    def close(self):
        """
        Closes the connection to the Braid proxy.
        """
        if self.stream:
            self.stream.close()
        self.session.close()