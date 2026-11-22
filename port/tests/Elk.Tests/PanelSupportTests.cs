using System.Text;
using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class PanelSupportTests
{
    [Fact]
    public void Name_Setter_Keeps_Existing_Space_Padding_When_Unchanged()
    {
        var a = new AreaRecord(); var padded = Encoding.ASCII.GetBytes("Area 1          "); padded.CopyTo(a.Bytes, 0);
        var before = (byte[])a.Bytes.Clone(); a.Name = "Area 1";
        Assert.Equal(before, a.Bytes);                              // unchanged text must not alter the record (or its CRC)
        a.Name = "Hall";
        Assert.Equal((byte)' ', a.Bytes[15]);                      // new text keeps the field's space padding
        Assert.Equal("Hall", a.Name);
    }

    [Fact]
    public void Name_Setter_Keeps_Nul_Padding_For_Nul_Padded_Fields()
    {
        var z = new ZoneRecord(); z.Name = "Front Door";
        Assert.Equal((byte)0, z.Bytes[4 + 15]);
        z.Name = "Front Door"; Assert.Equal("Front Door", z.Name);
    }

    private static string? Testing(string name)
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "TESTING", name); if (File.Exists(p)) return File.ReadAllText(p, Encoding.Latin1); d = d.Parent; }
        return null;
    }

    [Fact]
    public void SetGlobals_Changes_CodeLength_And_Roundtrips()
    {
        var text = Testing("RHTEST"); if (text == null) return;
        var f = AccountFile.Parse(text);
        var g = f.Globals; Assert.NotNull(g); Assert.Equal(48, g!.Length); Assert.Equal(4, f.CodeLength);
        g[0] |= 4; f.SetGlobals(g);
        Assert.Equal(6, f.CodeLength);
        var f2 = AccountFile.Parse(f.ToText());
        Assert.Equal(6, f2.CodeLength); Assert.Equal(g, f2.Globals);
        g[0] = (byte)(g[0] & ~4); f.SetGlobals(g);
        Assert.Equal(text, f.ToText());                              // setting the original values back restores the file exactly
    }
}
