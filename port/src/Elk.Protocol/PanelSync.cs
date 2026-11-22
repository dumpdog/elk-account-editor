namespace Elk.Protocol;

/// <summary>One record that differs between an account file and the panel.</summary>
public sealed record PanelChange(string Label, int Number, byte Section, byte[] Data, bool IsKeypad = false, byte Part = 0);

public sealed class SyncResult
{
    public List<PanelChange> Changes { get; } = new();
    /// <summary>Per section: records already identical, records that differ.</summary>
    public SortedDictionary<string, (int Same, int Differ)> Counts { get; } = new();
    public List<string> Skipped { get; } = new();
}

/// <summary>Connects with the same retry the CLI uses (the panel keeps a previous session open for a while after a write).</summary>
public static class PanelConnection
{
    public sealed record Settings(string Host, int Port, string Code, bool Plain, string? Mac = null, string? Passphrase = null, bool Tls = false, string? Serial = null);

    public static async Task<ElkSession> OpenAsync(Settings c, Action<string>? log = null, int attempts = 6, int delayMs = 5000, CancellationToken ct = default)
    {
        for (int attempt = 1; ; attempt++)
        {
            var s = c.Plain ? new ElkSession() : c.Tls ? ElkSession.Tls(c.Serial ?? throw new ArgumentException("The panel serial number is needed for the secure port")) : new ElkSession(c.Mac ?? throw new ArgumentException("MAC address is needed for the AES port"), c.Passphrase ?? "");
            try
            {
                await s.ConnectAsync(c.Host, c.Port, ct);
                await s.LoginAsync(c.Code, ct);
                return s;
            }
            catch (Exception e) when (attempt < attempts && e is TimeoutException or IOException or System.Net.Sockets.SocketException)
            {
                s.Dispose();
                log?.Invoke($"login attempt {attempt} failed ({e.GetType().Name}); retrying in {delayMs / 1000} s");
                await Task.Delay(delayMs, ct);
            }
            catch { s.Dispose(); throw; }
        }
    }
}

/// <summary>Compares an ElkRP account file with the panel record by record and writes what differs.
/// Only records present in the file are touched. Globals and dawn/dusk are not sent; users and user report codes above 199 have no panel item.</summary>
public static class PanelSync
{
    /// <summary>How many records or blocks <see cref="DiffAsync"/> will read (a keypad is two transfers). Used for the progress bar.</summary>
    public static int EstimateDiffTransfers(AccountFile f, bool resetMissing)
    {
        static int Upto(AccountFile.Section s, int max) => s.Entries.Count(e => e.Number <= max);
        static int Missing(AccountFile.Section s, int from, int to) => (to - from + 1) - s.Entries.Count(e => e.Number >= from && e.Number <= to);
        int n = Upto(f.Users, 199) + Upto(f.Areas, 8) + Upto(f.Zones, 208) + 2 * f.Keypads.Entries.Count + Upto(f.Phones, 8) + (f.Wireless?.Length ?? 0) + (f.Globals is { Length: 48 } ? 1 : 0);
        if (f.HasComm) n += Upto(f.AreaRcs, 8) + Upto(f.UserRcs, 199) + 1 + Upto(f.ZoneRcs, 208) + 1;
        if (resetMissing)
        {
            n += Missing(f.Zones, 1, 208) + Missing(f.Users, 2, 199) + Missing(f.Areas, 2, 8) + Missing(f.Phones, 1, 8);
            if (f.HasComm) n += Missing(f.AreaRcs, 2, 8) + Missing(f.UserRcs, 2, 199) + Missing(f.ZoneRcs, 1, 208);
            n += 2 * Missing(f.Keypads, 2, 16);
        }
        return n;
    }

    /// <summary>The transfers <see cref="PullAsync"/> makes before it reads the report codes (their number is added when the records are known).</summary>
    public static int EstimatePullTransfers(AccountFile f) => (f.Wireless?.Length ?? 0) + (f.Globals != null ? 1 : 0) + 208 + 2 * 16 + 8 + 199 + (f.HasPhoneSection ? 8 : 0);

    /// <summary>The transfers needed to write these changes (a keypad is two).</summary>
    public static int WriteTransfers(IEnumerable<PanelChange> changes) => changes.Sum(c => c.IsKeypad ? 2 : 1);

    /// <param name="resetMissing">Also compare panel records the file does not contain with factory defaults (zones, users 2+, areas 2+, keypads 2+, telephones and the report codes), so a smaller account clears what it dropped.</param>
    public static async Task<SyncResult> DiffAsync(ElkSession s, AccountFile f, bool resetMissing = false, bool sendLocalCode = false, CancellationToken ct = default)
    {
        var r = new SyncResult();
        async Task Block(string label, byte sec, int num, byte[] want, int max, byte part = 0)
        {
            if (num > max) { r.Skipped.Add($"{label} {num}"); return; }
            var have = await s.ReadBlockAsync(sec, (byte)num, part, ct);
            var c = r.Counts.GetValueOrDefault(label);
            if (have.AsSpan().SequenceEqual(want)) { r.Counts[label] = (c.Same + 1, c.Differ); return; }
            r.Counts[label] = (c.Same, c.Differ + 1);
            r.Changes.Add(new PanelChange(label, num, sec, want, Part: part));
        }
        foreach (var e in f.Users.Entries) await Block("user", 1, e.Number, e.Bytes, 199);
        foreach (var e in f.Areas.Entries) await Block("area", 3, e.Number, e.Bytes, 8);
        foreach (var e in f.Zones.Entries) await Block("zone", 5, e.Number, e.Bytes, 208);
        foreach (var e in f.Keypads.Entries)
        {
            var have = await s.ReadKeypadAsync((byte)e.Number, ct); var c = r.Counts.GetValueOrDefault("keypad");
            if (have.Bytes.AsSpan().SequenceEqual(e.Bytes)) { r.Counts["keypad"] = (c.Same + 1, c.Differ); continue; }
            r.Counts["keypad"] = (c.Same, c.Differ + 1);
            r.Changes.Add(new PanelChange("keypad", e.Number, 4, e.Bytes, IsKeypad: true));
        }
        foreach (var e in f.Phones.Entries) await Block("telephone", 8, e.Number, e.Bytes, 8);
        if (f.Wireless is { } fw) for (int i = 0; i < fw.Length; i++) await Block("wireless block " + (i + 1), 15, 1, fw[i], 1, (byte)i);     // on the panel the part selects the block; the item number is ignored (it mirrors)
        if (f.Globals is { Length: 48 } fg)
        {
            // Globals: send the file's settings. The local (keypad) programming code (bytes 17-22, factory default 172839) stays as the panel has it unless sendLocalCode is set.
            var have = await s.ReadBlockAsync(7, 1, 0, ct); var want = (byte[])have.Clone();
            for (int i = 0; i < 48; i++) if (sendLocalCode || i < 17 || i > 22) want[i] = fg[i];
            var c = r.Counts.GetValueOrDefault("globals");
            if (have.AsSpan().SequenceEqual(want)) r.Counts["globals"] = (c.Same + 1, c.Differ);
            else { r.Counts["globals"] = (c.Same, c.Differ + 1); r.Changes.Add(new PanelChange("globals", 1, 7, want)); }
        }
        if (f.HasComm)
        {
            foreach (var e in f.AreaRcs.Entries) await Block("area report codes", 9, e.Number, e.Bytes, 8);
            foreach (var e in f.UserRcs.Entries) await Block("user report codes", 10, e.Number, e.Bytes, 199);
            await Block("system report codes", 11, 1, f.SystemRc, 1);
            foreach (var e in f.ZoneRcs.Entries) await Block("zone report codes", 12, e.Number, e.Bytes, 208);
            await Block("keypad report codes", 16, 1, f.KeypadRc, 1);
        }
        if (resetMissing)
        {
            async Task Reset(string label, byte sec, int from, int to, ISet<int> present, Func<int, byte[]> def)
            {
                label += " (reset)";
                for (int n = from; n <= to; n++) if (!present.Contains(n)) await Block(label, sec, n, def(n), to);
            }
            ISet<int> Nums(AccountFile.Section sc) => sc.Entries.Select(e => e.Number).ToHashSet();
            await Reset("zone", 5, 1, 208, Nums(f.Zones), DefaultRecords.Zone);
            await Reset("user", 1, 2, 199, Nums(f.Users), DefaultRecords.User);
            await Reset("area", 3, 2, 8, Nums(f.Areas), DefaultRecords.Area);
            await Reset("telephone", 8, 1, 8, Nums(f.Phones), _ => DefaultRecords.Phone());
            if (f.HasComm)
            {
                await Reset("area report codes", 9, 2, 8, Nums(f.AreaRcs), _ => DefaultRecords.AreaRc());
                await Reset("user report codes", 10, 2, 199, Nums(f.UserRcs), _ => DefaultRecords.UserRc());
                await Reset("zone report codes", 12, 1, 208, Nums(f.ZoneRcs), _ => DefaultRecords.ZoneRc());
            }
            var have = Nums(f.Keypads);
            for (int n = 2; n <= 16; n++)
            {
                if (have.Contains(n)) continue;
                var k = await s.ReadKeypadAsync((byte)n, ct); var c = r.Counts.GetValueOrDefault("keypad (reset)"); var want = DefaultRecords.Keypad();
                if (k.Bytes.AsSpan().SequenceEqual(want)) { r.Counts["keypad (reset)"] = (c.Same + 1, c.Differ); continue; }
                r.Counts["keypad (reset)"] = (c.Same, c.Differ + 1);
                r.Changes.Add(new PanelChange("keypad (reset)", n, 4, want, IsKeypad: true));
            }
        }
        return r;
    }

    /// <summary>Reads the panel into <paramref name="f"/>: records already in the file are overwritten with the panel's; a record that is not in the file is added only
    /// when the panel's copy is clearly in use (a non-default name, a code, a number, a non-zero zone). Returns a note per section.</summary>
    public static async Task<List<string>> PullAsync(ElkSession s, AccountFile f, CancellationToken ct = default, PanelProgress? progress = null)
    {
        var notes = new List<string>(); int upd, add;
        static string Name(byte[] b, int off, int len = 16) => System.Text.Encoding.ASCII.GetString(b, off, len).TrimEnd(' ', ' ');
        async Task<byte[]> Read(byte sec, int item, byte part = 0) => await s.ReadBlockAsync(sec, (byte)item, part, ct);

        if (f.Wireless is { } pw) { for (int i = 0; i < pw.Length; i++) f.SetWireless(i, await Read(15, 1, (byte)i)); notes.Add("wireless receiver blocks"); }
        if (f.Globals != null) { var g = await Read(7, 1); f.SetGlobals(g); notes.Add($"globals: {(f.CodeLength == 6 ? "6" : "4")}-digit user codes"); }

        upd = add = 0;
        for (int n = 1; n <= 208; n++)
        {
            var have = await Read(5, n); var e = f.Zones.Get(n);
            if (e == null) { if (have[0] == 0 && (Name(have, 4) == "" || Name(have, 4) == $"Zone {n:D3}")) continue; e = f.Zones.GetOrAdd(n); add++; } else upd++;   // an undefined zone with a blank or default name is not brought in
            e.Bytes = have;
        }
        notes.Add($"zones: {upd} updated, {add} added");

        upd = add = 0;
        for (int n = 1; n <= 16; n++)
        {
            var k = await s.ReadKeypadAsync((byte)n, ct); var e = f.Keypads.Get(n);
            if (e == null) { if (k.Bytes[0] == 0 || Name(k.Bytes, 6) == "") continue; e = f.Keypads.GetOrAdd(n); add++; } else upd++;   // no area = not in use
            e.Bytes = (byte[])k.Bytes.Clone();
        }
        notes.Add($"keypads: {upd} updated, {add} added");

        // an area is brought in when the file has it, or when it is renamed or used by a zone or keypad
        var used = new HashSet<int>(f.Keypads.Entries.Select(k => (int)k.Bytes[0]));
        foreach (var z in f.Zones.Entries) if (z.Bytes[0] != 0) used.Add(((z.Bytes[2] >> 4) & 7) + 1);
        upd = add = 0;
        for (int n = 1; n <= 8; n++)
        {
            var have = await Read(3, n); var e = f.Areas.Get(n);
            if (e == null) { if (!used.Contains(n) && Name(have, 0) == $"Area {n}") continue; e = f.Areas.GetOrAdd(n); add++; } else upd++;
            e.Bytes = have;
        }
        notes.Add($"areas: {upd} updated, {add} added");

        upd = add = 0;
        for (int n = 1; n <= 199; n++)
        {
            var e = f.Users.Get(n);
            var have = await Read(1, n);
            if (e == null) { if (have.Take(6).All(b => b == 0)) continue; e = f.Users.GetOrAdd(n); add++; } else upd++;
            e.Bytes = have;
        }
        notes.Add($"users: {upd} updated, {add} added (users with an all-zero code are not added)");

        if (f.HasPhoneSection)
        {
            upd = add = 0;
            for (int n = 1; n <= 8; n++)
            {
                var have = await Read(8, n); var e = f.Phones.Get(n);
                if (e == null) { if (have.Take(20).All(b => b == 0) && (have[21] >> 4) == 0) continue; e = f.Phones.GetOrAdd(n); add++; } else upd++;
                e.Bytes = have;
            }
            notes.Add($"telephones: {upd} updated, {add} added");
        }

        if (f.HasComm)
        {
            progress?.AddTotal(f.Areas.Entries.Count + f.Users.Entries.Count(u => u.Number <= 199) + f.Zones.Entries.Count + 2);     // the report codes: one read per record now in the file, plus system and keypad
            foreach (var a in f.Areas.Entries.ToList()) f.AreaRcs.GetOrAdd(a.Number).Bytes = await Read(9, a.Number);
            foreach (var u in f.Users.Entries.Where(u => u.Number <= 199).ToList()) f.UserRcs.GetOrAdd(u.Number).Bytes = await Read(10, u.Number);
            foreach (var z in f.Zones.Entries.ToList()) f.ZoneRcs.GetOrAdd(z.Number).Bytes = await Read(12, z.Number);
            (await Read(11, 1)).CopyTo(f.SystemRc, 0);
            (await Read(16, 1)).CopyTo(f.KeypadRc, 0);
            notes.Add($"report codes: {f.AreaRcs.Entries.Count} areas, {f.UserRcs.Entries.Count} users, {f.ZoneRcs.Entries.Count} zones, system and keypad panic");
        }
        return notes;
    }

    public static async Task ApplyAsync(ElkSession s, IEnumerable<PanelChange> changes, Action<PanelChange>? progress = null, CancellationToken ct = default)
    {
        foreach (var c in changes)
        {
            if (c.IsKeypad) { var k = new KeypadRecord(); c.Data.CopyTo(k.Bytes, 0); await s.WriteKeypadAsync((byte)c.Number, k, ct); }
            else await s.WriteBlockAsync(c.Section, (byte)c.Number, c.Data, c.Part, ct);
            progress?.Invoke(c);
        }
    }
}
