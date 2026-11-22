using Elk.Protocol;

/// <summary>
/// One panel session that stays open between requests. The M1XEP accepts a single programming connection at a time, so while this is open every other
/// panel action (compare, send, read, backup, restore) borrows it instead of logging in again, and the Status tab polls it.
/// Requests are serialised with a lock; when the connection breaks it is closed and <see cref="Error"/> says why.
/// </summary>
sealed class LivePanel
{
    private readonly SemaphoreSlim _gate = new(1, 1);
    private ElkSession? _s;
    private PanelConnection.Settings? _settings;

    public bool Connected => _s != null;
    public string? Host => _settings?.Host;
    public string? Error { get; private set; }
    public string? Firmware => _s?.Panel?.Firmware;
    public string? Serial => _s?.Panel?.SerialHex;

    private static bool IsLinkError(Exception e) => e is TimeoutException or IOException or System.Net.Sockets.SocketException or ObjectDisposedException or InvalidDataException;
    private static bool Same(PanelConnection.Settings a, PanelConnection.Settings b) => a.Host == b.Host && a.Port == b.Port;

    private async Task CloseLocked()
    {
        var s = _s; _s = null;
        if (s != null) { try { await s.DisconnectAsync(); } catch { } s.Dispose(); }
    }

    public async Task ConnectAsync(PanelConnection.Settings st, Action<string> log)
    {
        await _gate.WaitAsync();
        try { await CloseLocked(); Error = null; _s = await PanelConnection.OpenAsync(st, log); _settings = st; }
        finally { _gate.Release(); }
    }

    public async Task DisconnectAsync()
    {
        await _gate.WaitAsync();
        try { await CloseLocked(); Error = null; } finally { _gate.Release(); }
    }

    /// <summary>Runs <paramref name="work"/> on the open session when it is for the same panel, otherwise on a session opened (and closed) just for this call.</summary>
    public async Task<T> UseAsync<T>(PanelConnection.Settings st, Action<string> log, Func<ElkSession, Task<T>> work)
    {
        await _gate.WaitAsync();
        try
        {
            if (_s != null && _settings != null && Same(_settings, st))
            {
                try { return await work(_s); }
                catch (Exception e) when (IsLinkError(e)) { Error = e.Message; await CloseLocked(); throw; }
            }
            using var s = await PanelConnection.OpenAsync(st, log);
            try { return await work(s); } finally { try { await s.DisconnectAsync(); } catch { } }
        }
        finally { _gate.Release(); }
    }

    /// <summary>Runs <paramref name="work"/> on the open session; throws InvalidOperationException when there is none.</summary>
    public async Task<T> UseOpenAsync<T>(Func<ElkSession, Task<T>> work)
    {
        await _gate.WaitAsync();
        try
        {
            if (_s == null) throw new InvalidOperationException("Not connected. Connect on the Status tab first.");
            try { return await work(_s); }
            catch (Exception e) when (IsLinkError(e)) { Error = e.Message; await CloseLocked(); throw; }
        }
        finally { _gate.Release(); }
    }

    /// <summary>Changes the RP access code on the panel and, once the panel acknowledges, remembers the new code so a reconnect after a dropped link uses it.</summary>
    public async Task<bool> ChangeAccessCodeAsync(string newCode)
    {
        await _gate.WaitAsync();
        try
        {
            if (_s == null || _settings == null) throw new InvalidOperationException("Not connected. Connect on the Live tab first.");
            bool ok;
            try { ok = await PanelAccess.ChangeAsync(_s, newCode); }
            catch (Exception e) when (IsLinkError(e)) { Error = e.Message; await CloseLocked(); throw; }
            if (ok) _settings = _settings with { Code = newCode };
            return ok;
        }
        finally { _gate.Release(); }
    }

    /// <summary>Starts bus module enrollment on the open session, waits for the panel to finish scanning, makes sure the session still works (logs in again if the panel dropped it) and lists the devices.</summary>
    public async Task<(string Note, List<BusModule> Modules)> EnrollAsync(Action<string> log)
    {
        await _gate.WaitAsync();
        try
        {
            if (_s == null || _settings == null) throw new InvalidOperationException("Not connected. Connect on the Status tab first.");
            var note = await PanelModules.EnrollAsync(_s);
            await Task.Delay(12000);
            try { await _s.ReadBlockAsync(7, 1); }
            catch (Exception e) when (IsLinkError(e)) { await CloseLocked(); _s = await PanelConnection.OpenAsync(_settings, log); }
            return (note, await PanelModules.ScanAsync(_s));
        }
        finally { _gate.Release(); }
    }

    /// <summary>Reads the live status; null when not connected (or the connection just broke).</summary>
    public async Task<LiveSnapshot?> PollAsync()
    {
        await _gate.WaitAsync();
        try
        {
            if (_s == null) return null;
            try { return await PanelLive.ReadAsync(_s); }
            catch (Exception e) when (IsLinkError(e)) { Error = e.Message; await CloseLocked(); return null; }
        }
        finally { _gate.Release(); }
    }
}
