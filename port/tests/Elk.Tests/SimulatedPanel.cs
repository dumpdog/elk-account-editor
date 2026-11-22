using System.Net;
using System.Net.Sockets;
using Elk.Protocol;

namespace Elk.Tests;

/// <summary>A software stand-in for a panel that follows the protocol exactly as traced from ElkRP's code.
/// It exercises framing, AES, partial-block reads and the request/response formats. It cannot prove the real
/// panel agrees, so passing tests here mean "consistent with ElkRP's code", not "works on hardware".</summary>
public sealed class SimulatedPanel : IDisposable
{
    private readonly TcpListener _l = new(IPAddress.Loopback, 0);
    private readonly byte[] _key;
    private string _accessCode;
    public string AccessCode => _accessCode;
    public DateTime Clock { get; set; } = new DateTime(2026, 11, 12, 19, 45, 30);
    /// <summary>Log entries as the 8-byte packed records the panel returns, newest first.</summary>
    public List<byte[]> LogRecords { get; } = new();
    private static readonly byte[] AckMarker = { 0x10, 0x06 };
    public int Port => ((IPEndPoint)_l.LocalEndpoint).Port;
    public Dictionary<(Section, byte), byte[]> Records { get; } = new();
    public List<byte[]> Received { get; } = new();
    public bool SplitWrites { get; set; }
    private readonly Task _run;
    private readonly CancellationTokenSource _cts = new();

    public SimulatedPanel(string mac, string passphrase, string accessCode)
    {
        _key = ElkCrypto.DeriveKey(mac, passphrase);
        _accessCode = accessCode;
        _l.Start();
        _run = Task.Run(Serve);
    }

    private async Task Serve()
    {
        try
        {
            using var c = await _l.AcceptTcpClientAsync(_cts.Token);
            var net = c.GetStream();
            var parser = new FrameParser(ffIsDebugText: false);   // the PC sends write requests that start with 0xFF
            var buf = new byte[2048];
            var pending = new List<byte>();
            while (!_cts.IsCancellationRequested)
            {
                int n = await net.ReadAsync(buf, _cts.Token);
                if (n == 0) break;
                pending.AddRange(buf.AsSpan(0, n).ToArray());
                int whole = pending.Count / 16 * 16;
                if (whole == 0) continue;
                var chunk = pending.GetRange(0, whole).ToArray(); pending.RemoveRange(0, whole);
                foreach (var f in parser.Feed(ElkCrypto.Decrypt(_key, chunk)))
                {
                    if (f.Kind != FrameKind.Message || !f.CrcOk) continue;
                    Received.Add(f.Payload);
                    var reply = Handle(f.Payload);
                    if (reply == AckMarker) await net.WriteAsync(ElkCrypto.Encrypt(_key, AckMarker));
                    else if (reply != null) await Send(net, reply);
                }
            }
        }
        catch (OperationCanceledException) { }
        catch (IOException) { }
    }

    private async Task Send(NetworkStream net, byte[] payload)
    {
        var enc = ElkCrypto.Encrypt(_key, Framing.Encode(payload));
        if (SplitWrites) { await net.WriteAsync(enc.AsMemory(0, 5)); await Task.Delay(30); await net.WriteAsync(enc.AsMemory(5)); }
        else await net.WriteAsync(enc);
    }

    private byte[]? Handle(byte[] p)
    {
        if (p[0] == 0x7F && p[1] == 0 && p[3] == 1)
        {
            var code = new string(Enumerable.Range(0, 6).Select(i => (char)('0' + p[9 - i])).ToArray()); // digits arrive last-first
            var r = new byte[25];
            r[0] = 0x7F; r[1] = (byte)(code == _accessCode ? 0 : 1);
            r[4] = 5; r[5] = 2; r[6] = 9;                      // firmware 5.2.9
            r[8] = 0x31; r[9] = 0x41; r[10] = 0x59; r[11] = 0x26; // serial 31415926
            r[12] = 1; r[13] = 2; r[14] = 3; r[15] = 4; r[16] = 5;
            return r;
        }
        if (p[0] == 0x7F && p.Length == 4 && p[1] == 0 && p[2] == 0 && p[3] is 6 or 7 or 8 or 16) return LiveReply(p[3]);   // status queries
        if (p[0] == 0x7F && p.Length == 4 && p[1] == 0 && p[2] == 0 && p[3] == 3)               // read the clock
        {
            static byte B(int v) => (byte)(((v / 10) << 4) | (v % 10));
            var c = Clock; return new byte[] { 0x7F, 0, 0, 3, B(c.Second), B(c.Minute), B(c.Hour), (byte)((int)c.DayOfWeek + 1), B(c.Day), B(c.Month), B(c.Year - 2000) };
        }
        if (p[0] == 0xFF && p.Length == 11 && p[3] == 3)                                       // set the clock: BCD fields, then ACK
        {
            static int D(byte b) => (b >> 4) * 10 + (b & 15);
            Clock = new DateTime(D(p[10]) + 2000, D(p[9]), D(p[8]), D(p[6]), D(p[5]), D(p[4])); return AckMarker;
        }
        if (p[0] == 0x7F && p.Length == 10 && p[3] == 5)                                       // change the RP access code: digits last first, then ACK
        {
            _accessCode = new string(Enumerable.Range(0, 6).Select(i => (char)('0' + p[9 - i])).ToArray()); return AckMarker;
        }
        if (p[0] == 0x7B && p.Length == 4)                                                     // event log: 00 = start, 14 hi lo = next 20 entries
        {
            if (p[1] == 0) return new byte[] { 0x7B, 0, 0, 0 };
            int start = p[2] * 256 + p[3] == 65535 ? 0 : p[2] * 256 + p[3], take = Math.Min(20, Math.Max(0, LogRecords.Count - start));
            var r = new List<byte> { 0x7B, (byte)take, (byte)((start + take) / 256), (byte)((start + take) & 0xFF) };
            foreach (var e in LogRecords.Skip(start).Take(take)) r.AddRange(e);
            return r.ToArray();
        }
        if (p[0] == 0x7F && p.Length == 6 && p[2] == 0x40) { ArmRequests.Add(p.ToArray()); return null; }   // arm / disarm: recorded, no reply
        if (p[0] == 0x7F) return null; // keepalive / disconnect: no reply
        if (p[0] == 0x7E)
        {
            var sec = (Section)p[1];
            var rec = Records[(sec, sec == Section.WirelessZone ? (byte)(p[2 - 1] + 1) : p[3])];
            var crc = Crc16.Compute(rec);
            return new byte[] { 0x7E, p[1], 0, p[3], (byte)(crc >> 8), (byte)crc };
        }
        if (p[0] >= 128)
        {
            var key = ((Section)(p[0] - 128), (Section)(p[0] - 128) == Section.WirelessZone ? (byte)(p[1] + 1) : p[3]);     // wireless: the part selects the block, the item is ignored
            if (key.Item1 == Section.Keypad)
            {
                var full = Records.TryGetValue(key, out var old) ? old : new byte[148];
                p[4..].CopyTo(full, p[1] == 0 ? 0 : 28);          // part 0 -> bytes 0..27, part 1 -> bytes 28..147
                Records[key] = full;
            }
            else Records[key] = p[4..];
            return new byte[] { p[0], p[1], p[2], p[3] };
        }
        var data = Records[((Section)p[0], (Section)p[0] == Section.WirelessZone ? (byte)(p[1] + 1) : p[3])];
        if ((Section)p[0] == Section.Keypad) data = p[1] == 0 ? data[..28] : data[28..];
        return new byte[] { p[0], p[1], 0, p[3] }.Concat(data).ToArray();
    }

    /// <summary>Canned status replies shaped like the real panel's: area 1 not ready, areas 2-8 ready; zones 1-16 open/violated; no system troubles.</summary>
    public List<byte[]> ArmRequests { get; } = new();
    /// <summary>Area status bytes to report instead of the defaults (bits 2-4: 0 not ready, 4 ready, 8 ready to force arm).</summary>
    public Dictionary<int, byte> AreaStatus { get; } = new();

    private byte[] LiveReply(byte n)
    {
        var r = new byte[n == 6 ? 68 : n == 7 ? 56 : 212]; r[0] = 0x7F; r[3] = n;
        if (n == 6) { for (int a = 2; a <= 8; a++) r[8 * a] = 0x04; r[8 * 3] = 0x24; foreach (var (a, v) in AreaStatus) r[8 * a] = v; }       // area 3 armed away, ready
        if (n == 8) for (int z = 1; z <= 16; z++) r[z + 3] = 0x09;
        if (n == 16) for (int z = 1; z <= 16; z++) r[z + 3] = 0x89;
        if (n == 7) r[4 + 18] = 1;                                                           // A.C. failure
        return r;
    }

    public void Dispose() { _cts.Cancel(); _l.Stop(); try { _run.Wait(500); } catch { } }
}
