using System.Text;
using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class DeleteTests
{
    private static string? Sample(string name)
    {
        var d = new DirectoryInfo(AppContext.BaseDirectory);
        while (d != null) { var p = Path.Combine(d.FullName, "sample", name); if (File.Exists(p)) return File.ReadAllText(p, Encoding.Latin1); d = d.Parent; }
        return null;
    }

    // Records captured from a factory-default panel (spare test panel, firmware 5.3.44)
    [Fact]
    public void Default_Records_Match_A_Real_Default_Panel()
    {
        Assert.Equal("000100005A6F6E6520313030000000000000000001D700160031000000000000", Convert.ToHexString(DefaultRecords.Zone(100)));
        Assert.Equal("000100005A6F6E6520323038000000000000000001D700170031001D00000000", Convert.ToHexString(DefaultRecords.Zone(208)));
        Assert.Equal("000000000000E2000155534552203130300000000000000000000000", Convert.ToHexString(DefaultRecords.User(100)));
        Assert.Equal("417265612035202020202020202020203C1E3C1E61500F00", Convert.ToHexString(DefaultRecords.Area(5)));
        Assert.Equal("0000000033600000", Convert.ToHexString(DefaultRecords.Keypad()[..8]));
        Assert.Equal(148, DefaultRecords.Keypad().Length);
        var ph = DefaultRecords.Phone(); Assert.Equal(88, ph.Length); Assert.Equal(0xA8, ph[20]); Assert.Equal(8, ph[22]); Assert.Equal((byte)'0', ph[23]); Assert.Equal((byte)'0', ph[70]); Assert.Equal(0, ph[71]);
        Assert.Equal("01000001", Convert.ToHexString(DefaultRecords.UserRc()));
    }

    [Fact]
    public void Without_Prune_A_Missing_Record_Is_Kept_And_With_Prune_It_Is_Removed()
    {
        var text = Sample("Sample Communicator"); if (text == null) return;
        var f = AccountFile.Parse(text); var m = AccountModel.From(f);
        int zones = m.Zones.Count, rcs = m.Comm!.Zones.Count;
        m.Zones.RemoveAll(z => z.N == 5 || z.N == 6); m.Comm.Zones.RemoveAll(z => z.N == 5 || z.N == 6);
        m.ApplyTo(f);                                                           // Prune off: nothing is deleted
        Assert.Equal(zones, f.Zones.Entries.Count); Assert.Equal(rcs, f.ZoneRcs.Entries.Count);

        m.Prune = true; m.ApplyTo(f);
        Assert.Equal(zones - 2, f.Zones.Entries.Count); Assert.Equal(rcs - 2, f.ZoneRcs.Entries.Count);
        Assert.Null(f.Zones.Get(5)); Assert.Null(f.ZoneRcs.Get(6));
        var f2 = AccountFile.Parse(f.ToText());                                  // still a valid file with the right counts
        Assert.Equal(zones - 2, f2.Zones.Entries.Count); Assert.True(f2.HasComm); Assert.Equal(rcs - 2, f2.ZoneRcs.Entries.Count);
        Assert.Equal(m.Phones.Count, AccountModel.From(f2).Phones.Count);
    }

    [Fact]
    public void Prune_Removes_Users_Phones_Keypads_And_Their_Report_Rows()
    {
        var text = Sample("Sample Communicator"); if (text == null) return;
        var f = AccountFile.Parse(text); var m = AccountModel.From(f);
        int users = m.Users.Count; int urc = m.Comm!.Users.Count;
        var victim = m.Users.First(u => u.N > 1).N;
        m.Users.RemoveAll(u => u.N == victim); m.Comm.Users.RemoveAll(u => u.N == victim);
        m.Phones.Clear();
        m.Prune = true; m.ApplyTo(f);
        Assert.Equal(users - 1, f.Users.Entries.Count); Assert.Equal(urc - 1, f.UserRcs.Entries.Count); Assert.Empty(f.Phones.Entries);
        var f2 = AccountFile.Parse(f.ToText());
        Assert.True(f2.HasComm); Assert.Empty(f2.Phones.Entries); Assert.Equal(users - 1, f2.Users.Entries.Count);
    }

    [Fact]
    public void Prune_Refuses_To_Leave_Things_In_A_Deleted_Area_Or_Delete_Required_Items()
    {
        var text = Sample("Sample Communicator"); if (text == null) return;
        var f = AccountFile.Parse(text); var m = AccountModel.From(f);
        var m1 = AccountModel.From(f); m1.Prune = true; m1.Areas.Clear();
        Assert.Throws<FormatException>(() => m1.ApplyTo(AccountFile.Parse(text)));
        var m2 = AccountModel.From(f); m2.Prune = true; m2.Users.RemoveAll(u => u.N == 1);
        Assert.Throws<FormatException>(() => m2.ApplyTo(AccountFile.Parse(text)));
        var m3 = AccountModel.From(f); m3.Prune = true; m3.Keypads.Clear();
        Assert.Throws<FormatException>(() => m3.ApplyTo(AccountFile.Parse(text)));
    }
}
