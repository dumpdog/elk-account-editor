using System.Text;
using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

/// <summary>Wireless receiver blocks. The real-file checks use ElkRP exports from the TESTING folder (R5-B Wireless: four devices on zones 33-36; R7-B: zone 33 changed from option 1 to option 2) and skip when absent.</summary>
public class WirelessTests
{
    private static string? Testing(string name)
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "TESTING", "ROUND5", name); if (File.Exists(p)) return File.ReadAllText(p, Encoding.Latin1); d = d.Parent; }
        return null;
    }
    private static string FindTemplate()
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "src", "Elk.Editor", "template-m1g.txt"); if (File.Exists(p)) return p; d = d.Parent; }
        throw new FileNotFoundException("template-m1g.txt");
    }

    [Fact]
    public void Template_Has_Three_Blocks_And_Unchanged_Apply_Is_Identical()
    {
        var text = File.ReadAllText(FindTemplate(), Encoding.Latin1);
        var f = AccountFile.Parse(text); var m = AccountModel.From(f);
        Assert.Equal(3, m.Wireless!.Length); Assert.All(m.Wireless, b => Assert.Equal(260, b.Length));
        m.ApplyTo(f); Assert.Equal(text, f.ToText());
        m.Wireless[0][4 + 16] |= 1; m.Wireless[0][116 + 3 * 16 + 2] = 0x35;
        m.ApplyTo(f);
        var w = AccountFile.Parse(f.ToText()).Wireless!;
        Assert.Equal(1, w[0][20] & 1); Assert.Equal(0x35, w[0][166]);
    }

    [Fact]
    public void Real_Exports_Decode_As_Documented()
    {
        var a = Testing("R5-B Wireless"); var b = Testing("R7-B"); if (a == null || b == null) return;
        var wa = AccountFile.Parse(a).Wireless![0]; var wb = AccountFile.Parse(b).Wireless![0];
        Assert.Equal(16, wa[2]);                                  // start zone 17
        Assert.Equal(41, wa[4 + 16]); Assert.Equal(49, wb[4 + 16]);   // zone 33: enabled+PIR+option 1  ->  enabled+PIR+option 2
        Assert.Equal(1, (wa[20] & 1)); Assert.True((wa[20] & 8) != 0 && (wa[20] & 16) == 0); Assert.True((wb[20] & 8) == 0 && (wb[20] & 16) != 0);
        Assert.Equal(new[] { 51, 52, 53, 53 }, new[] { 0, 1, 2, 3 }.Select(i => (int)wa[116 + 3 * (16 + i) + 2]).ToArray());
    }
}
