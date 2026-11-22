using System.Text;
using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

/// <summary>Diff / apply / pull against the in-process simulated panel (see SimulatedPanel: consistent with ElkRP's code, not proof about hardware).</summary>
public class PanelSyncTests
{
    private const string Mac = "00:40:9D:00:00:01", Pass = "testphrase", Code = "246801";

    private static string? Sample(string name)
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "sample", name); if (File.Exists(p)) return File.ReadAllText(p, Encoding.Latin1); d = d.Parent; }
        return null;
    }

    private static void Put(SimulatedPanel p, int section, int item, byte[] data) => p.Records[((Section)section, (byte)item)] = (byte[])data.Clone();

    /// <summary>Fills the simulated panel with every record the sync code may read: zeros by default, then the file's own records.</summary>
    private static void Seed(SimulatedPanel p, AccountFile f)
    {
        for (int n = 1; n <= 199; n++) { Put(p, 1, n, new byte[28]); Put(p, 10, n, new byte[4]); }
        for (int n = 1; n <= 8; n++) { Put(p, 3, n, new byte[24]); Put(p, 8, n, new byte[88]); Put(p, 9, n, new byte[20]); }
        for (int n = 1; n <= 16; n++) Put(p, 4, n, new byte[148]);
        for (int n = 1; n <= 208; n++) { Put(p, 5, n, new byte[32]); Put(p, 12, n, new byte[4]); }
        if (f.Wireless is { } fw) for (int i = 0; i < fw.Length; i++) Put(p, 15, i + 1, fw[i]);
        Put(p, 7, 1, f.Globals ?? new byte[48]); Put(p, 11, 1, new byte[24]); Put(p, 16, 1, new byte[96]);
        foreach (var e in f.Users.Entries.Where(e => e.Number <= 199)) Put(p, 1, e.Number, e.Bytes);
        foreach (var e in f.Areas.Entries) Put(p, 3, e.Number, e.Bytes);
        foreach (var e in f.Keypads.Entries) Put(p, 4, e.Number, e.Bytes);
        foreach (var e in f.Zones.Entries) Put(p, 5, e.Number, e.Bytes);
        foreach (var e in f.Phones.Entries) Put(p, 8, e.Number, e.Bytes);
        foreach (var e in f.AreaRcs.Entries) Put(p, 9, e.Number, e.Bytes);
        foreach (var e in f.UserRcs.Entries.Where(e => e.Number <= 199)) Put(p, 10, e.Number, e.Bytes);
        Put(p, 11, 1, f.SystemRc);
        foreach (var e in f.ZoneRcs.Entries) Put(p, 12, e.Number, e.Bytes);
        Put(p, 16, 1, f.KeypadRc);
    }

    private static async Task<ElkSession> Connect(SimulatedPanel p)
    {
        var s = new ElkSession(Mac, Pass);
        await s.ConnectAsync("127.0.0.1", p.Port);
        await s.LoginAsync(Code);
        return s;
    }

    [Fact]
    public async Task Diff_Of_Identical_Panel_And_File_Has_No_Changes()
    {
        var text = Sample("Sample Communicator"); if (text == null) return;
        var f = AccountFile.Parse(text);
        using var panel = new SimulatedPanel(Mac, Pass, Code); Seed(panel, f);
        using var s = await Connect(panel);
        var r = await PanelSync.DiffAsync(s, f);
        Assert.Empty(r.Changes);
        Assert.Contains("zone", r.Counts.Keys); Assert.Equal(0, r.Counts["zone"].Differ); Assert.True(r.Counts["zone"].Same > 0);
        Assert.Contains("user 201", r.Skipped);                               // special users have no panel item
    }

    [Fact]
    public async Task Diff_Apply_Diff_Writes_Only_What_Changed_And_Converges()
    {
        var text = Sample("Sample Communicator"); if (text == null) return;
        var f = AccountFile.Parse(text);
        using var panel = new SimulatedPanel(Mac, Pass, Code); Seed(panel, f);
        // edit the file in memory: a zone name, the phone number, a zone report row, an area name and the keypad name
        var m = AccountModel.From(f);
        m.Zones[0].Name = "Edited Zone"; m.Phones[0].Number = "5550000"; m.Comm!.Zones[0].Codes[0] = 0; m.Areas[0].Name = "Edited Area"; m.Keypads[0].Name = "Edited Pad";
        m.ApplyTo(f);

        using var s = await Connect(panel);
        var d1 = await PanelSync.DiffAsync(s, f);
        Assert.Equal(5, d1.Changes.Count);
        Assert.Equal(new[] { "area", "keypad", "telephone", "zone", "zone report codes" }, d1.Changes.Select(c => c.Label).OrderBy(x => x).ToArray());

        await PanelSync.ApplyAsync(s, d1.Changes);
        var d2 = await PanelSync.DiffAsync(s, f);
        Assert.Empty(d2.Changes);                                             // read back: panel now equals the file
        Assert.Equal("Edited Zone", new ZoneRecord().Also(z => { f.Zones.Entries[0].Bytes.CopyTo(z.Bytes, 0); }).Name);
        Assert.Equal(f.Zones.Entries[0].Bytes, panel.Records[(Section.Zone, (byte)f.Zones.Entries[0].Number)]);
        Assert.Equal(f.Phones.Entries[0].Bytes, panel.Records[(Section.Telephone, (byte)1)]);
        Assert.Equal(f.Keypads.Entries[0].Bytes, panel.Records[(Section.Keypad, (byte)1)]);
    }

    [Fact]
    public async Task Pull_Overwrites_File_Records_And_Adds_Only_Used_Ones()
    {
        var text = Sample("Sample Communicator"); if (text == null) return;
        var f = AccountFile.Parse(text);
        using var panel = new SimulatedPanel(Mac, Pass, Code); Seed(panel, f);

        // the panel has a changed zone, an extra zone in use, an unused default-looking zone, an extra user with a code, and a user without one
        var z3 = (byte[])panel.Records[(Section.Zone, (byte)3)].Clone(); Array.Clear(z3, 4, 16); Encoding.ASCII.GetBytes("Panel Name").CopyTo(z3, 4); Put(panel, 5, 3, z3);
        var zNew = new byte[32]; zNew[0] = 1; Encoding.ASCII.GetBytes("New Zone").CopyTo(zNew, 4); Put(panel, 5, 150, zNew);
        var zDefault = new byte[32]; Encoding.ASCII.GetBytes("Zone 151").CopyTo(zDefault, 4); Put(panel, 5, 151, zDefault);
        var u = new byte[28]; u[0] = 1; u[1] = 2; u[6] = 0xE4; Encoding.ASCII.GetBytes("PULLED").CopyTo(u, 9); Put(panel, 1, 50, u);
        Put(panel, 1, 51, new byte[28]);

        using var s = await Connect(panel);
        var notes = await PanelSync.PullAsync(s, f);
        Assert.NotEmpty(notes);

        Assert.Equal("Panel Name", AccountModel.From(f).Zones.First(z => z.N == 3).Name);
        Assert.NotNull(f.Zones.Get(150)); Assert.Null(f.Zones.Get(151));
        Assert.NotNull(f.Users.Get(50)); Assert.Null(f.Users.Get(51));
        Assert.Equal("PULLED", AccountModel.From(f).Users.First(x => x.N == 50).Name);
        Assert.NotNull(f.ZoneRcs.Get(150));                                   // pulled zones get report-code rows
        // the result is a valid file again, and a diff against the same panel is clean
        var f2 = AccountFile.Parse(f.ToText());
        var d = await PanelSync.DiffAsync(s, f2);
        Assert.Empty(d.Changes);
    }

    [Fact]
    public async Task Progress_Estimates_Match_The_Transfers_Actually_Made()
    {
        var text = Sample("Sample Communicator"); if (text == null) return;
        var f = AccountFile.Parse(text);
        using var panel = new SimulatedPanel(Mac, Pass, Code); Seed(panel, f);
        using var s = await Connect(panel);
        int n = 0; s.OnTransfer = () => n++;
        await PanelSync.DiffAsync(s, f);
        Assert.Equal(PanelSync.EstimateDiffTransfers(f, false), n);
        n = 0; await PanelSync.DiffAsync(s, f, resetMissing: true);
        Assert.Equal(PanelSync.EstimateDiffTransfers(f, true), n);
        // a write is one transfer (two for a keypad)
        n = 0; var changes = new List<PanelChange> { new("zone", 1, 5, new byte[32]), new("keypad", 1, 4, new byte[148], IsKeypad: true) };
        await PanelSync.ApplyAsync(s, changes);
        Assert.Equal(PanelSync.WriteTransfers(changes), n); Assert.Equal(3, n);
    }

    [Fact]
    public async Task Pull_Progress_Reaches_Exactly_Its_Total()
    {
        var text = Sample("Sample Communicator"); if (text == null) return;
        var f = AccountFile.Parse(text);
        using var panel = new SimulatedPanel(Mac, Pass, Code); Seed(panel, f);
        using var s = await Connect(panel);
        var pr = new PanelProgress(); pr.Begin("Reading the panel", PanelSync.EstimatePullTransfers(f)); s.OnTransfer = pr.Tick;
        await PanelSync.PullAsync(s, f, default, pr);
        var snap = pr.Snapshot(); Assert.True(snap.Active); Assert.Equal(snap.Total, snap.Done); Assert.True(snap.Done > 400);
        pr.End(); pr.Tick(); Assert.Equal(snap.Done, pr.Snapshot().Done);          // nothing counts once the operation has ended
    }

    [Fact]
    public void Progress_Total_Grows_If_More_Happens_Than_Expected()
    {
        var pr = new PanelProgress(); pr.Tick(); Assert.Equal(0, pr.Snapshot().Done);   // before Begin: ignored
        pr.Begin("x", 2); pr.Tick(); pr.Tick(); pr.Tick(); var s = pr.Snapshot(); Assert.Equal((3, 3), (s.Done, s.Total));
        pr.Next("y", 0); pr.Tick(); s = pr.Snapshot(); Assert.Equal(("y", 1, 0), (s.Phase, s.Done, s.Total));   // 0 = unknown total stays unknown
    }
}

internal static class TestExt { public static T Also<T>(this T t, Action<T> a) { a(t); return t; } }

public class PanelResetTests
{
    private const string Mac = "00:40:9D:00:00:02", Pass = "testphrase", Code = "246801";
    private static void Put(SimulatedPanel p, int section, int item, byte[] data) => p.Records[((Section)section, (byte)item)] = (byte[])data.Clone();

    /// <summary>A panel in factory-default state: every record is the default one.</summary>
    private static void SeedDefaults(SimulatedPanel p)
    {
        for (int n = 1; n <= 199; n++) { Put(p, 1, n, DefaultRecords.User(n)); Put(p, 10, n, DefaultRecords.UserRc()); }
        for (int n = 1; n <= 8; n++) { Put(p, 3, n, DefaultRecords.Area(n)); Put(p, 8, n, DefaultRecords.Phone()); Put(p, 9, n, DefaultRecords.AreaRc()); }
        for (int n = 1; n <= 16; n++) Put(p, 4, n, DefaultRecords.Keypad());
        for (int n = 1; n <= 208; n++) { Put(p, 5, n, DefaultRecords.Zone(n)); Put(p, 12, n, DefaultRecords.ZoneRc()); }
        Put(p, 7, 1, new byte[48]); Put(p, 11, 1, new byte[24]); Put(p, 16, 1, new byte[96]);
        for (int i = 1; i <= 3; i++) Put(p, 15, i, new byte[260]);
    }

    [Fact]
    public async Task ResetMissing_Only_Touches_Records_That_Differ_From_Default_And_Are_Not_In_The_File()
    {
        var f = AccountFile.Parse(File.ReadAllText(FindTemplate(), Encoding.Latin1));      // 48 zones, 4 users, 1 area, 1 keypad
        using var panel = new SimulatedPanel(Mac, Pass, Code); SeedDefaults(panel);
        // leftovers on the panel that the file does not have: a custom zone 150, a user 50 with a code, a phone, area 3
        var z = new ZoneRecord(); z.Name = "Old Zone"; z.Function = 1; Put(panel, 5, 150, z.Bytes);
        var u = DefaultRecords.User(50); u[0] = 1; u[1] = 2; Put(panel, 1, 50, u);
        var ph = DefaultRecords.Phone(); ph[0] = (byte)'5'; Put(panel, 8, 2, ph);
        var a = DefaultRecords.Area(3); a[0] = (byte)'X'; Put(panel, 3, 3, a);

        using var s = new ElkSession(Mac, Pass); await s.ConnectAsync("127.0.0.1", panel.Port); await s.LoginAsync(Code);
        var plain = await PanelSync.DiffAsync(s, f, resetMissing: false);
        Assert.DoesNotContain(plain.Changes, c => c.Label.EndsWith("(reset)"));          // off by default

        var d = await PanelSync.DiffAsync(s, f, resetMissing: true);
        var reset = d.Changes.Where(c => c.Label.EndsWith("(reset)")).Select(c => $"{c.Label} {c.Number}").OrderBy(x => x).ToArray();
        Assert.Equal(new[] { "area (reset) 3", "telephone (reset) 2", "user (reset) 50", "zone (reset) 150" }, reset);

        await PanelSync.ApplyAsync(s, d.Changes.Where(c => c.Label.EndsWith("(reset)")));
        var after = await PanelSync.DiffAsync(s, f, resetMissing: true);
        Assert.DoesNotContain(after.Changes, c => c.Label.EndsWith("(reset)"));          // panel records outside the file are default again
        Assert.Equal(DefaultRecords.Zone(150), panel.Records[(Section.Zone, (byte)150)]);
    }

    private static string FindTemplate()
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "src", "Elk.Editor", "template-m1g.txt"); if (File.Exists(p)) return p; d = d.Parent; }
        throw new FileNotFoundException("template-m1g.txt");
    }

}
