using System.Text;
using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class GlobalsTests
{
    private const string Mac = "00:40:9D:00:00:03", Pass = "testphrase", Code = "246801";
    private static string FindTemplate()
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "src", "Elk.Editor", "template-m1g.txt"); if (File.Exists(p)) return p; d = d.Parent; }
        throw new FileNotFoundException("template-m1g.txt");
    }

    [Fact]
    public void Globals_Edit_Round_Trips_And_Unchanged_Is_Byte_Identical()
    {
        var text = File.ReadAllText(FindTemplate(), Encoding.Latin1);
        var f = AccountFile.Parse(text);
        var m = AccountModel.From(f);
        Assert.NotNull(m.Globals); Assert.Equal(48, m.Globals!.Length);
        m.ApplyTo(f); Assert.Equal(text, f.ToText());                       // untouched = identical
        m.Globals[16] = 7; m.Globals[0] |= 8;                               // wrong-code lockout, audible trouble
        m.ApplyTo(f);
        var g = AccountFile.Parse(f.ToText()).Globals!;
        Assert.Equal(7, g[16]); Assert.True((g[0] & 8) != 0);
    }

    [Fact]
    public async Task Globals_Are_Sent_Without_Touching_The_Panels_Local_Code_Unless_Asked()
    {
        var f = AccountFile.Parse(File.ReadAllText(FindTemplate(), Encoding.Latin1));
        using var panel = new SimulatedPanel(Mac, Pass, Code);
        void P(int sec, int n, byte[] b) => panel.Records[((Section)sec, (byte)n)] = (byte[])b.Clone();
        foreach (var e in f.Users.Entries.Where(e => e.Number <= 199)) P(1, e.Number, e.Bytes);
        foreach (var e in f.Areas.Entries) P(3, e.Number, e.Bytes);
        foreach (var e in f.Zones.Entries) P(5, e.Number, e.Bytes);
        foreach (var e in f.Phones.Entries) P(8, e.Number, e.Bytes);
        foreach (var e in f.AreaRcs.Entries) P(9, e.Number, e.Bytes);
        foreach (var e in f.UserRcs.Entries.Where(e => e.Number <= 199)) P(10, e.Number, e.Bytes);
        foreach (var e in f.ZoneRcs.Entries) P(12, e.Number, e.Bytes);
        for (int i = 0; i < f.Wireless!.Length; i++) P(15, i + 1, f.Wireless[i]);
        P(11, 1, f.SystemRc); P(16, 1, f.KeypadRc);
        foreach (var e in f.Keypads.Entries) P(4, e.Number, e.Bytes);
        var onPanel = new byte[48]; for (int i = 17; i <= 22; i++) onPanel[i] = 9; onPanel[47] = 5; onPanel[16] = 3;
        panel.Records[((Section)7, (byte)1)] = (byte[])onPanel.Clone();
        var want = f.Globals!; want[16] = 6;
        f.SetGlobals(want);
        using var s = new ElkSession(Mac, Pass); await s.ConnectAsync("127.0.0.1", panel.Port); await s.LoginAsync(Code);
        var d = await PanelSync.DiffAsync(s, f);
        var ch = Assert.Single(d.Changes, c => c.Label == "globals");
        await PanelSync.ApplyAsync(s, new[] { ch });
        var now = panel.Records[((Section)7, (byte)1)];
        Assert.Equal(6, now[16]);                                           // setting applied
        for (int i = 17; i <= 22; i++) Assert.Equal(9, now[i]);             // local programming code kept
        var d2 = await PanelSync.DiffAsync(s, f, sendLocalCode: true);          // opted in: the file's local code is now a difference too
        var ch2 = Assert.Single(d2.Changes, c => c.Label == "globals");
        await PanelSync.ApplyAsync(s, new[] { ch2 });
        var sent = panel.Records[((Section)7, (byte)1)];
        for (int i = 17; i <= 22; i++) Assert.Equal(f.Globals![i], sent[i]);
        Assert.Equal(f.Globals![47], now[47]);                              // country and modem values are sent
    }
}
