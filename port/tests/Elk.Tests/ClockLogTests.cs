using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class ClockLogTests
{
    private const string Mac = "00:40:9D:11:22:33", Pass = "elk", Code = "246801";

    [Fact]
    public void Set_Request_Is_Bcd_With_Sunday_As_One()
    {
        var t = new DateTime(2026, 11, 12, 19, 45, 30);                    // a Thursday: 5
        Assert.Equal(new byte[] { 0xFF, 0, 0, 3, 0x30, 0x45, 0x19, 5, 0x12, 0x11, 0x26 }, PanelClock.SetRequest(t));
        Assert.Equal(1, PanelClock.SetRequest(new DateTime(2026, 11, 15, 0, 0, 0))[7]);   // a Sunday
    }

    [Fact]
    public void Clock_Reply_Decodes_And_A_Zero_Day_Or_Month_Becomes_One()
    {
        var r = new byte[] { 0x7F, 0, 0, 3, 0x30, 0x45, 0x19, 5, 0x12, 0x11, 0x26 };
        Assert.Equal(new DateTime(2026, 11, 12, 19, 45, 30), PanelClock.Decode(r));
        r[8] = 0; r[9] = 0; Assert.Equal(new DateTime(2026, 1, 1, 19, 45, 30), PanelClock.Decode(r));
        r[6] = 0x99; Assert.Null(PanelClock.Decode(r));
        Assert.Null(PanelClock.Decode(new byte[5]));
    }

    [Fact]
    public void Access_Code_Request_Sends_The_Digits_Last_First()
    {
        Assert.Equal(new byte[] { 0x7F, 0, 0, 5, 1, 0, 8, 6, 4, 2 }, PanelAccess.ChangeRequest("246801"));
        Assert.Equal(new byte[] { 0x7F, 0, 0, 5, 6, 5, 4, 3, 2, 1 }, PanelAccess.ChangeRequest("123456"));
        foreach (var bad in new[] { "", "12345", "1234567", "12a456", "12 456", "١٢٣٤٥٦" }) Assert.Throws<ArgumentException>(() => PanelAccess.ChangeRequest(bad));
    }

    /// <summary>Packs an entry the way the panel does (see PanelLog's description).</summary>
    public static byte[] Pack(DateTime t, int area, int ev, int item)
    {
        int y = t.Year - 2000, mo = t.Month, d = t.Day, h = t.Hour, mi = t.Minute, dow = (int)t.DayOfWeek + 1, a = area - 1, i = item - 1;
        return new byte[]
        {
            (byte)((y << 3) | (mo >> 1)), (byte)(((mo & 1) << 7) | (d << 2) | (h >> 3)), (byte)(((h & 7) << 5) | (mi >> 1)), (byte)(((mi & 1) << 7) | (dow << 4) | (a << 1)),
            (byte)(ev >> 4), (byte)(((ev & 15) << 4) | (i >> 5)), (byte)((i & 31) << 3), 1,
        };
    }

    [Fact]
    public void Log_Entry_Round_Trips_Through_The_Packing()
    {
        var t = new DateTime(2026, 11, 12, 19, 45, 0);
        var e = PanelLog.DecodeEntry(Pack(t, 3, 1367, 17), 0, 1)!;
        Assert.Equal(t, e.Time); Assert.Equal(3, e.Area); Assert.Equal(1367, e.Event); Assert.Equal(17, e.Item);
        Assert.Null(PanelLog.DecodeEntry(new byte[8], 0, 1));                                       // all zero: empty
        Assert.True(PanelLog.DecodeEntry(Pack(t, 1, 4095, 1), 0, 1)!.EndOfLog);
    }

    [Fact]
    public void End_Of_Log_Marker_In_The_Middle_Is_Skipped_And_An_Empty_Slot_Ends_The_List()
    {
        var t = new DateTime(2026, 11, 16, 22, 41, 0);
        var r = new List<byte> { 0x7B, 5, 0x01, 0xED };
        r.AddRange(Pack(t, 1, 1363, 1)); r.AddRange(Pack(t, 1, 4095, 1)); r.AddRange(Pack(t.AddDays(-5), 2, 1174, 1)); r.AddRange(new byte[8]); r.AddRange(Pack(t, 1, 1111, 1));
        var (entries, next, ended) = PanelLog.DecodeReply(r.ToArray(), 1);
        Assert.Equal(new[] { 1363, 1174 }, entries.Select(e => e.Event)); Assert.Equal(new[] { 1, 2 }, entries.Select(e => e.Index));
        Assert.True(ended); Assert.Equal(0x01ED, next);
        Assert.Single(PanelLog.DecodeReply(r.ToArray(), 1, maxSlots: 1).Entries);        // only the first slot is looked at
    }

    [Fact]
    public async Task Clock_Can_Be_Read_And_Set_Over_A_Session()
    {
        using var panel = new SimulatedPanel(Mac, Pass, Code);
        using var s = new ElkSession(Mac, Pass); await s.ConnectAsync("127.0.0.1", panel.Port); await s.LoginAsync(Code);
        Assert.Equal(panel.Clock, await PanelClock.ReadAsync(s));
        var want = new DateTime(2027, 3, 9, 6, 7, 8);
        Assert.True(await PanelClock.SetAsync(s, want));
        Assert.Equal(want, panel.Clock); Assert.Equal(want, await PanelClock.ReadAsync(s));
    }

    [Fact]
    public async Task Access_Code_Change_Is_Acknowledged_And_The_New_Code_Works_Afterwards()
    {
        using var panel = new SimulatedPanel(Mac, Pass, Code);
        using (var s = new ElkSession(Mac, Pass)) { await s.ConnectAsync("127.0.0.1", panel.Port); await s.LoginAsync(Code); Assert.True(await PanelAccess.ChangeAsync(s, "135790")); }
        Assert.Equal("135790", panel.AccessCode);
    }

    [Fact]
    public async Task Log_Is_Read_Newest_First_In_Blocks_Of_Twenty()
    {
        using var panel = new SimulatedPanel(Mac, Pass, Code);
        var t0 = new DateTime(2026, 11, 12, 10, 0, 0);
        for (int i = 0; i < 45; i++) panel.LogRecords.Add(Pack(t0.AddMinutes(-i), 1 + i % 8, 100 + i, 1 + i));
        using var s = new ElkSession(Mac, Pass); await s.ConnectAsync("127.0.0.1", panel.Port); await s.LoginAsync(Code);
        var all = await PanelLog.ReadAsync(s, 100);
        Assert.Equal(45, all.Count); Assert.Equal(new[] { 1, 2, 3 }, all.Take(3).Select(e => e.Index)); Assert.Equal(100, all[0].Event); Assert.Equal(t0.AddMinutes(-44), all[44].Time);
        Assert.Equal(7, (await PanelLog.ReadAsync(s, 7)).Count);                                     // a smaller request stops early
    }
}
