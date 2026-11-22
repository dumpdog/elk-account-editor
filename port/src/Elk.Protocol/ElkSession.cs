using System.Net.Security;
using System.Net.Sockets;
using System.Security.Authentication;
using System.Security.Cryptography.X509Certificates;

namespace Elk.Protocol;

/// <summary>AES-over-TCP session (ElkRP ConnectMethod 51). Framed + AES-ECB encrypted request/response.
/// Tested only against the in-process simulated panel in the test project, NOT against real hardware.</summary>
public sealed class ElkSession : IDisposable
{
    private readonly TcpClient _tcp = new();
    private Stream? _net;
    private readonly bool _tls;             // M1XEP secure port: TLS, no passphrase (ElkRP ConnectMethod 50)
    private readonly string? _serialHex;   // the control's 8-digit serial number, used in the TLS login block
    private readonly byte[]? _key;    // null = plain (non-secure) network, ElkRP ConnectMethod 48
    private readonly FrameParser _parser = new();
    private readonly Queue<Frame> _frames = new();
    private readonly List<byte> _cipherBuf = new();

    public ConnectReply? Panel { get; private set; }
    public TimeSpan Timeout { get; set; } = TimeSpan.FromMilliseconds(3000);

    public ElkSession(string macAddress, string passphrase) => _key = ElkCrypto.DeriveKey(macAddress, passphrase);

    /// <summary>Plain (non-secure) network session: the same framing with no AES layer. Untested against hardware.</summary>
    public ElkSession() => _key = null;

    private ElkSession(string serialHex) { _key = null; _tls = true; _serialHex = serialHex; }

    /// <summary>M1XEP secure port (usually 2601): the same framing inside a TLS stream. No passphrase, user name or password is involved; the login carries a block built from the
    /// control's serial number (8 hex digits). The module's certificate is self-signed, so only the two issuers ElkRP itself accepts are trusted.</summary>
    public static ElkSession Tls(string serialHex)
        => serialHex.Length == 8 && serialHex.All(Uri.IsHexDigit) ? new ElkSession(serialHex) : throw new ArgumentException("The panel serial number must be 8 hex digits.");

    private static bool TrustModule(object sender, X509Certificate? cert, X509Chain? chain, SslPolicyErrors errors)
        => cert != null && (cert.Issuer == "CN=M1XEP, OU=Engineering, O=Elk, L=Hildebran, S=NC, C=US" || cert.Issuer == "CN=HTTPS Certificate" || errors == SslPolicyErrors.None);

    public async Task ConnectAsync(string host, int port, CancellationToken ct = default)
    {
        await _tcp.ConnectAsync(host, port, ct);
        if (!_tls) { _net = _tcp.GetStream(); return; }
        var ssl = new SslStream(_tcp.GetStream(), false, TrustModule);
        try
        {
            await ssl.AuthenticateAsClientAsync(new SslClientAuthenticationOptions { TargetHost = "M1XEP", EnabledSslProtocols = SslProtocols.Tls12, CertificateRevocationCheckMode = X509RevocationMode.NoCheck }, ct);
        }
        catch (AuthenticationException e) { ssl.Dispose(); throw new IOException("TLS handshake with the panel failed: " + e.Message + (e.InnerException != null ? " (" + e.InnerException.Message + ")" : ""), e); }
        _net = ssl;
    }

    /// <summary>Connect then log in with the six-digit RP access code. Throws if the panel does not accept it.</summary>
    public async Task<ConnectReply> LoginAsync(string accessCode, CancellationToken ct = default)
    {
        await SendAsync(_tls ? Handshake.BuildConnect(accessCode, ConnectMode.Tls, _serialHex) : Handshake.BuildConnect(accessCode), ct);
        var reply = ConnectReply.Parse(await ReceiveAsync(Handshake.SessionOpcode, ct));
        if (!reply.Accepted) throw new UnauthorizedAccessException("Panel rejected the RP access code.");
        return Panel = reply;
    }

    public async Task SendAsync(byte[] payload, CancellationToken ct = default)
        => await _net!.WriteAsync(_key == null ? Framing.Encode(payload) : ElkCrypto.Encrypt(_key, Framing.Encode(payload)), ct);

    /// <summary>Wait for a good message whose first byte equals <paramref name="expect"/>. Partial AES blocks are buffered across reads.</summary>
    public async Task<byte[]> ReceiveAsync(byte expect, CancellationToken ct = default)
        => (await ReceiveFrameWhereAsync(f => f.Kind == FrameKind.Message && f.CrcOk && f.Payload.Length > 0 && f.Payload[0] == expect, "opcode 0x" + expect.ToString("X2"), ct)).Payload;

    /// <summary>Wait for the next good message whatever its first byte is (used for the remote-control commands, whose reply opcode is not fixed).</summary>
    public async Task<byte[]> ReceiveAnyAsync(CancellationToken ct = default)
        => (await ReceiveFrameWhereAsync(f => f.Kind == FrameKind.Message && f.CrcOk && f.Payload.Length > 0, "any message", ct)).Payload;

    /// <summary>Wait for the next frame of any kind: a message, or the bare acknowledgement (10 06) / refusal (10 15) the panel sends for some commands.</summary>
    public Task<Frame> ReceiveAnyFrameAsync(CancellationToken ct = default) => ReceiveFrameWhereAsync(_ => true, "any frame", ct);

    private async Task<Frame> ReceiveFrameWhereAsync(Func<Frame, bool> accept, string what, CancellationToken ct)
    {
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        cts.CancelAfter(Timeout);
        var buf = new byte[2048];
        while (true)
        {
            while (_frames.Count > 0)
            {
                var f = _frames.Dequeue();
                if (accept(f)) return f;
            }
            int n;
            try { n = await _net!.ReadAsync(buf, cts.Token); }
            catch (OperationCanceledException) when (!ct.IsCancellationRequested) { throw new TimeoutException($"No reply for {what}."); }
            if (n == 0) throw new IOException("Connection closed by panel.");
            if (_key == null) { foreach (var f in _parser.Feed(buf.AsSpan(0, n).ToArray())) _frames.Enqueue(f); continue; }
            _cipherBuf.AddRange(buf.AsSpan(0, n).ToArray());
            int whole = _cipherBuf.Count / 16 * 16;
            if (whole == 0) continue;
            var chunk = _cipherBuf.GetRange(0, whole).ToArray();
            _cipherBuf.RemoveRange(0, whole);
            foreach (var f in _parser.Feed(ElkCrypto.Decrypt(_key, chunk))) _frames.Enqueue(f);
        }
    }

    /// <summary>Called after every record or block read or written (the progress counter hangs on this).</summary>
    public Action? OnTransfer { get; set; }

    /// <summary>Read one record. Request [section,0,0,item]; the reply echoes the section id, record bytes start at offset 4.</summary>
    public async Task<byte[]> ReadRecordAsync(Section s, byte item, int size, CancellationToken ct = default, byte part = 0)
    {
        await SendAsync(Handshake.ReadRequest(s, item, part), ct);
        var r = await ReceiveAsync((byte)s, ct);
        if (r.Length < 4 + size) throw new InvalidDataException($"Short reply ({r.Length} bytes, wanted {4 + size}).");
        OnTransfer?.Invoke();
        return r[4..(4 + size)];
    }

    /// <summary>Write one record. Request [section+128,0,0,item,data...]; the panel echoes section+128.</summary>
    public async Task WriteRecordAsync(Section s, byte item, byte[] data, CancellationToken ct = default, byte part = 0)
    {
        await SendAsync(Handshake.WriteRequest(s, item, data, part), ct);
        await ReceiveAsync((byte)((byte)s + 128), ct);
        OnTransfer?.Invoke();
    }

    /// <summary>Read any section item. Returns everything after the 4-byte reply header [section][part][0][item].</summary>
    public async Task<byte[]> ReadBlockAsync(byte section, byte item, byte part = 0, CancellationToken ct = default)
    {
        await SendAsync(new byte[] { section, part, 0, item }, ct);
        var r = await ReceiveAsync(section, ct);
        OnTransfer?.Invoke();
        return r[4..];
    }

    /// <summary>Write any section item (opcode = section + 128). The panel echoes the opcode.</summary>
    public async Task WriteBlockAsync(byte section, byte item, byte[] data, byte part = 0, CancellationToken ct = default)
    {
        await SendAsync(Handshake.WriteRequest((Section)section, item, data, part), ct);
        await ReceiveAsync((byte)(section + 128), ct);
        OnTransfer?.Invoke();
    }

    /// <summary>The panel's stored CRC for an item (ElkRP compares it with its ModCRC/UDCRC to skip unchanged records).</summary>
    public async Task<ushort> QueryCrcAsync(Section s, byte item, CancellationToken ct = default)
    {
        await SendAsync(Handshake.CrcQuery(s, item), ct);
        var r = await ReceiveAsync(Handshake.CrcQueryOpcode, ct);
        return (ushort)((r[4] << 8) | r[5]);
    }

    public Task<ZoneRecord> ReadZoneAsync(byte zone, CancellationToken ct = default)
        => ReadRecordAsync(Section.Zone, zone, ZoneRecord.Size, ct).ContinueWith(t => ZoneRecord.FromBytes(t.Result), ct);

    public Task<AreaRecord> ReadAreaAsync(byte area, CancellationToken ct = default)
        => ReadRecordAsync(Section.Area, area, AreaRecord.Size, ct).ContinueWith(t => { var a = new AreaRecord(); t.Result.CopyTo(a.Bytes, 0); return a; }, ct);

    /// <summary>Keypads move in two parts: part 0 = 28 bytes, part 1 = 120 bytes.</summary>
    public async Task<KeypadRecord> ReadKeypadAsync(byte keypad, CancellationToken ct = default)
    {
        var k = new KeypadRecord();
        (await ReadRecordAsync(Section.Keypad, keypad, 28, ct, 0)).CopyTo(k.Bytes, 0);
        (await ReadRecordAsync(Section.Keypad, keypad, 120, ct, 1)).CopyTo(k.Bytes, 28);
        return k;
    }

    public async Task WriteKeypadAsync(byte keypad, KeypadRecord k, CancellationToken ct = default)
    {
        await WriteRecordAsync(Section.Keypad, keypad, k.Bytes[..28], ct, 0);
        await WriteRecordAsync(Section.Keypad, keypad, k.Bytes[28..], ct, 1);
    }

    public async Task KeepAliveAsync(CancellationToken ct = default) => await SendAsync(Handshake.KeepAlive, ct);

    public async Task DisconnectAsync(CancellationToken ct = default)
    {
        try { await SendAsync(Handshake.Disconnect, ct); } catch (IOException) { }
    }

    public void Dispose() => _tcp.Dispose();
}
