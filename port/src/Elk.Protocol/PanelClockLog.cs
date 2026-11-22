namespace Elk.Protocol;

/// <summary>
/// The panel's clock, from ElkRP's "Set Time and Date" window (SetTime.cs). Read: <c>7F 00 00 03</c>; the reply holds seconds, minutes, hours (BCD) at [4..6], the day of week (Sunday = 1)
/// at [7], then day, month and year minus 2000 (BCD) at [8..10]. Set: <c>FF 00 00 03</c> followed by the same seven bytes; the panel answers with an ACK.
/// </summary>
public static class PanelClock
{
    private static byte Bcd(int v) => (byte)(((v / 10) << 4) | (v % 10));
    private static int FromBcd(byte b) => (b >> 4) * 10 + (b & 15);

    public static byte[] SetRequest(DateTime t) => new byte[]
    {
        0xFF, 0, 0, 3, Bcd(t.Second), Bcd(t.Minute), Bcd(t.Hour), (byte)((int)t.DayOfWeek + 1), Bcd(t.Day), Bcd(t.Month), Bcd(t.Year - 2000),
    };

    /// <summary>Decodes a read reply; null if the panel's clock is not valid (day or month zero is treated as 1, as ElkRP does).</summary>
    public static DateTime? Decode(byte[] r)
    {
        if (r.Length < 11) return null;
        int day = FromBcd(r[8]), month = FromBcd(r[9]);
        try { return new DateTime(FromBcd(r[10]) + 2000, month == 0 ? 1 : month, day == 0 ? 1 : day, FromBcd(r[6]), FromBcd(r[5]), FromBcd(r[4])); }
        catch (ArgumentOutOfRangeException) { return null; }
    }

    public static async Task<DateTime?> ReadAsync(ElkSession s, CancellationToken ct = default)
    {
        await s.SendAsync(new byte[] { 0x7F, 0, 0, 3 }, ct);
        var r = await s.ReceiveAsync(0x7F, ct);
        return r.Length >= 11 && r[3] == 3 ? Decode(r) : throw new InvalidDataException("unexpected reply to the clock query");
    }

    /// <summary>Sets the clock and reports whether the panel acknowledged. Day of week follows the date given.</summary>
    public static async Task<bool> SetAsync(ElkSession s, DateTime t, CancellationToken ct = default)
    {
        if (t.Year is < 2000 or > 2099) throw new ArgumentOutOfRangeException(nameof(t), "the panel keeps years 2000 to 2099");
        await s.SendAsync(SetRequest(t), ct);
        try { return (await s.ReceiveAnyFrameAsync(ct)).Kind == FrameKind.Ack; } catch (TimeoutException) { return false; }
    }
}

/// <summary>
/// Changes the RP access code (the code ElkRP and this editor log in with), as the Change button in ElkRP's account details does: <c>7F 00 00 05</c> followed by the six digits,
/// last digit first (the same order as the login), answered with an ACK. The panel then expects the new code on the next login.
/// </summary>
public static class PanelAccess
{
    public static bool IsValid(string code) => code.Length == 6 && code.All(char.IsAsciiDigit);

    public static byte[] ChangeRequest(string newCode)
    {
        if (!IsValid(newCode)) throw new ArgumentException("The RP access code is exactly six digits.", nameof(newCode));
        var p = new byte[10]; p[0] = 0x7F; p[3] = 5;
        for (int n = 1; n <= 6; n++) p[10 - n] = (byte)(newCode[n - 1] - '0');      // digit 1 goes last, digit 6 first
        return p;
    }

    /// <summary>Sends the change and reports whether the panel acknowledged it.</summary>
    public static async Task<bool> ChangeAsync(ElkSession s, string newCode, CancellationToken ct = default)
    {
        await s.SendAsync(ChangeRequest(newCode), ct);
        try { return (await s.ReceiveAnyFrameAsync(ct)).Kind == FrameKind.Ack; } catch (TimeoutException) { return false; }
    }
}

/// <summary>One entry of the panel's event log. <see cref="Event"/> is the panel's event number; ElkRP names it from a table in its own database, which is not read here.</summary>
public sealed record LogEntry(int Index, DateTime? Time, int Area, int Event, int Item, bool EndOfLog);

/// <summary>
/// The event log, from ElkRP's "Receive Log" window (ReceiveLog.cs). Request <c>7B 14 hi lo</c> (20 entries, starting at address 65535 for the newest; ElkRP's extra opening request <c>7B 00 00 00</c> writes a marker into the log and is not used); the reply holds the number of entries
/// returned at [1], the address of the next block at [2..3] and 8 bytes per entry from [4]. An entry packs, from the first byte: year-2000 (5 bits), month (4), day (5), hour (5), minute (6),
/// day of week (3), area-1 (3), event number (12), item number-1 (9) and a 3-bit tail. Event 0 or an all-zero date ends the list; event 4095 is the "END OF LOG" marker, which the real panel returns in the middle of the list (older events follow), so it is skipped. The log holds 512 entries.
/// </summary>
public static class PanelLog
{
    public const int Capacity = 512;

    public static LogEntry? DecodeEntry(byte[] e, int offset, int index)
    {
        byte b0 = e[offset], b1 = e[offset + 1], b2 = e[offset + 2], b3 = e[offset + 3], b4 = e[offset + 4], b5 = e[offset + 5], b6 = e[offset + 6];
        int year = (b0 >> 3) & 0x1F, month = ((b0 & 7) << 1) | (b1 >> 7), day = (b1 >> 2) & 0x1F, hour = ((b1 & 3) << 3) | (b2 >> 5), minute = ((b2 & 0x1F) << 1) | (b3 >> 7);
        int area = ((b3 >> 1) & 7) + 1, ev = b4 * 16 + (b5 >> 4), item = (((b5 & 0xF) << 5) | ((b6 >> 3) & 0x1F)) + 1;
        if (ev == 0 || (year == 0 && month == 0 && day == 0)) return null;
        if (ev == 4095) return new LogEntry(index, null, area, ev, item, true);
        DateTime? t = null;
        try { t = new DateTime(2000 + year, month, day, hour, minute, 0); } catch (ArgumentOutOfRangeException) { }
        return new LogEntry(index, t, area, ev, item, false);
    }

    /// <summary>Decodes a log reply into its entries (stops at the first empty one).</summary>
    public static (List<LogEntry> Entries, int Next, bool Ended) DecodeReply(byte[] r, int firstIndex, int maxSlots = int.MaxValue)
    {
        var list = new List<LogEntry>(); bool ended = false;
        int count = Math.Min(r.Length > 1 ? r[1] : 0, maxSlots), next = r.Length > 3 ? r[2] * 256 + r[3] : 0;
        for (int i = 0; i < count && 4 + 8 * i + 7 < r.Length; i++)
        {
            var e = DecodeEntry(r, 4 + 8 * i, firstIndex + list.Count);
            if (e == null) { ended = true; break; }        // an empty slot: nothing older
            if (e.EndOfLog) continue;                       // event 4095 marks the write position (seen on the real panel with older events after it), it is not an event
            list.Add(e);
        }
        return (list, next, ended || count == 0);
    }

    /// <summary>Reads up to <paramref name="max"/> entries, newest first. Read only.</summary>
    public static async Task<List<LogEntry>> ReadAsync(ElkSession s, int max = 20, CancellationToken ct = default)
    {
        // ElkRP first sends 7B 00 00 00. On the real panel that request writes an END OF LOG marker (event 4095, stamped with the time) into the log and the newer events then sit
        // before it; it is a write, so it is not sent here. Reading from address 65535 returns the newest entries first without it.
        var all = new List<LogEntry>(); int next = 65535, slots = 0;
        while (all.Count < Math.Min(max, Capacity) && slots < Capacity)
        {
            await s.SendAsync(new byte[] { 0x7B, 20, (byte)(next / 256), (byte)(next & 0xFF) }, ct);
            var r = await s.ReceiveAsync(0x7B, ct);
            var (entries, n, ended) = DecodeReply(r, all.Count + 1, Capacity - slots);        // the log is a ring of 512 slots: read one lap, no more
            all.AddRange(entries.Take(Math.Min(max, Capacity) - all.Count));
            slots += r.Length > 1 ? r[1] : 0;
            if (ended || n == next) break;
            next = n;
        }
        return all;
    }
}
