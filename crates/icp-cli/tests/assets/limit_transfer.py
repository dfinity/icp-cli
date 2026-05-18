"""
mitmproxy addon that allows a limited number of request/response pairs through.
Usage: mitmdump --mode reverse:http://localhost:PORT -p PROXY_PORT -s limit_transfer.py
Set LIMIT_REQUESTS environment variable to control how many requests to allow (default: 2).

Requests are serialized - only one in flight at a time. After the limit is reached,
subsequent requests are killed.
Default of 2 allows metadata + one data chunk through.
"""

from mitmproxy import ctx, http
import os
import asyncio

class LimitTransfer:
    def __init__(self):
        self.requests_allowed = int(os.environ.get("LIMIT_REQUESTS", 2))
        self.requests_completed = 0
        self.in_flight = False
        self.waiters = []
        ctx.log.info(f"LimitTransfer: allowing {self.requests_allowed} requests")

    async def request(self, flow: http.HTTPFlow):
        # If limit reached, kill immediately
        if self.requests_completed >= self.requests_allowed:
            ctx.log.info(f"LimitTransfer: killing request (limit reached)")
            flow.kill()
            return

        # If another request is in flight, wait
        if self.in_flight:
            # Check if we'd exceed limit when this eventually runs
            if self.requests_completed + len(self.waiters) + 1 >= self.requests_allowed:
                ctx.log.info(f"LimitTransfer: killing request (would exceed limit)")
                flow.kill()
                return

            event = asyncio.Event()
            self.waiters.append(event)
            ctx.log.info(f"LimitTransfer: stalling request")
            await event.wait()

            # Check again after waking
            if self.requests_completed >= self.requests_allowed:
                ctx.log.info(f"LimitTransfer: killing request after wait (limit reached)")
                flow.kill()
                return

        self.in_flight = True
        ctx.log.info(f"LimitTransfer: allowing request ({self.requests_completed + 1}/{self.requests_allowed})")

    def response(self, flow: http.HTTPFlow):
        self.requests_completed += 1
        self.in_flight = False
        ctx.log.info(f"LimitTransfer: {self.requests_completed}/{self.requests_allowed} requests completed")

        # Wake next waiter if under limit
        if self.waiters and self.requests_completed < self.requests_allowed:
            waiter = self.waiters.pop(0)
            waiter.set()

addons = [LimitTransfer()]
