namespace Elk.Protocol;

public sealed record ProgressSnapshot(bool Active, string Phase, string Note, int Done, int Total, double Seconds);

/// <summary>
/// Progress of a long panel operation (read, compare, send, backup, restore). The session calls <see cref="Tick"/> after every record or block it reads or writes
/// (ElkSession.OnTransfer); the operation sets the phase and the number of transfers it expects with <see cref="Next"/>. A total of 0 means "not known" (the page shows
/// a moving bar and a count only). The counts are estimates that the code makes exact where it can; if more transfers happen than expected the total grows with them.
/// </summary>
public sealed class PanelProgress
{
    private readonly object _l = new();
    private string _phase = "", _note = ""; private int _done, _total; private bool _active; private DateTime _started;

    /// <summary>Starts an operation (the clock for "seconds so far" starts here).</summary>
    public void Begin(string phase, int total = 0) { lock (_l) { _active = true; _started = DateTime.UtcNow; _phase = phase; _note = ""; _done = 0; _total = total; } }

    /// <summary>Moves to the next phase of the same operation.</summary>
    public void Next(string phase, int total = 0) { lock (_l) { if (!_active) { _active = true; _started = DateTime.UtcNow; } _phase = phase; _note = ""; _done = 0; _total = total; } }

    public void AddTotal(int n) { lock (_l) _total += n; }
    public void Note(string text) { lock (_l) _note = text; }
    public void Tick() { lock (_l) { if (!_active) return; _done++; if (_total > 0 && _done > _total) _total = _done; } }
    public void End() { lock (_l) _active = false; }

    public ProgressSnapshot Snapshot() { lock (_l) return new ProgressSnapshot(_active, _phase, _note, _done, _total, _active ? (DateTime.UtcNow - _started).TotalSeconds : 0); }
}
