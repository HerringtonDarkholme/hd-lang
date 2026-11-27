# A real controlling terminal, with bounded waits and cleanup even on failure.
import os
import pty
import select
import signal
import sys
import time

pid, terminal = pty.fork()
if pid == 0:
    os.execv(sys.argv[1], [sys.argv[1], sys.argv[2], "--engine", sys.argv[3],
                          "--time-limit", "4s", "--", sys.argv[4]])
output = b""
sent_child = sent_parent = False
reaped = False
status = None
deadline = time.monotonic() + 7
try:
    while time.monotonic() < deadline:
        if select.select([terminal], [], [], 0.05)[0]:
            try:
                chunk = os.read(terminal, 65536)
                if not chunk:
                    break
                output += chunk
            except OSError:
                break
        if b"READY" in output and not sent_child:
            os.write(terminal, b"X\nY\n")
            sent_child = True
        if b"RESTORED" in output and not sent_parent:
            os.write(terminal, b"Z\n")
            sent_parent = True
        done, status = os.waitpid(pid, os.WNOHANG)
        if done:
            reaped = True
            assert os.waitstatus_to_exitcode(status) == 0, output
            break
    assert b"tty=true,true,true" in output, output
    assert b"TTY=X,Y" in output, output
    assert b"STATUS=7 RESTORED" in output, output
    assert b"parent=Z" in output, output
finally:
    if not reaped:
        until = time.monotonic() + 1
        while time.monotonic() < until:
            done, status = os.waitpid(pid, os.WNOHANG)
            if done:
                break
            time.sleep(0.01)
        else:
            os.kill(pid, signal.SIGKILL)
            _, status = os.waitpid(pid, 0)
    os.close(terminal)
assert os.waitstatus_to_exitcode(status) == 0, output
print(output.decode("utf8", errors="replace"))
