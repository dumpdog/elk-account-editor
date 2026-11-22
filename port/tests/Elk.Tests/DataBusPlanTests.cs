using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class DataBusPlanTests
{
    private static BusDevice D(string model, int addr, string name = "", double ma = 0) => new() { Model = model, Address = addr, Name = name, Ma = ma };
    private static BusRun R(string name, double ft, params BusDevice[] d) => new() { Name = name, LengthFt = ft, Devices = d.ToList() };
    private static MainRun Direct(double ft, params BusDevice[] d) => new() { Kind = "direct", Name = "Bus", Run = R("Bus", ft, d) };
    private static MainRun Dbh(double feed, params BusHub[] hubs) => new() { Kind = "dbh", FeedFt = feed, Hubs = hubs.ToList() };
    private static BusHub Hub(double crossover, params BusRun[] runs) => new() { CrossoverFt = crossover, HomeRuns = runs.ToList() };
    private static MainRun Dbhr(double feed, params BusRun[][] branches)
    {
        var m = new MainRun { Kind = "dbhr", FeedFt = feed };
        for (int i = 0; i < 4; i++) m.Branches.Add(new BusBranch { Runs = i < branches.Length ? branches[i].ToList() : new() });
        return m;
    }
    private static BusPlan P(params MainRun[] runs) => new() { Runs = runs.ToList() };
    private static bool Has(PlanResult r, string level, string text) => r.Issues.Any(i => i.Level == level && i.Text.Contains(text, StringComparison.OrdinalIgnoreCase));

    [Fact]
    public void A_Simple_Single_Run_Is_Clean_And_Needs_Jp3()
    {
        var r = DataBusPlanner.Check(P(Direct(150, D("Keypad", 1, "Hall"), D("M1XIN", 3, "Garage"))));
        Assert.Equal(0, r.Errors); Assert.Equal(0, r.Warnings); Assert.True(r.Jp3); Assert.Equal(150, r.MainBusFt); Assert.Equal(2, r.Devices);
        var c = string.Join("\n", r.Checklist.SelectMany(g => g.Items));
        Assert.Contains("JP3 (terminating jumper on the M1 control): INSTALL", c); Assert.Contains("Terminate the last device only: M1XIN \"Garage\" (address 3)", c);
    }

    [Fact]
    public void Two_Runs_Remove_Jp3_And_More_Than_Two_Is_An_Error()
    {
        var two = DataBusPlanner.Check(P(Direct(100, D("Keypad", 1)), Direct(100, D("Keypad", 2))));
        Assert.False(two.Jp3); Assert.Contains("REMOVE it", string.Join("\n", two.Checklist.SelectMany(g => g.Items)));
        var three = DataBusPlanner.Check(P(Direct(10, D("Keypad", 1)), Direct(10, D("Keypad", 2)), Direct(10, D("Keypad", 3))));
        Assert.True(Has(three, "error", "No more than 2 runs"));
    }

    [Fact]
    public void Addresses_May_Repeat_Across_Types_But_Not_Within_A_Type()
    {
        Assert.Equal(0, DataBusPlanner.Check(P(Direct(50, D("Keypad", 3), D("M1XIN", 3), D("M1XOVR", 3), D("M1XSP", 3)))).Errors);        // different types
        Assert.True(Has(DataBusPlanner.Check(P(Direct(50, D("Keypad", 4), D("M1KAM", 4)))), "error", "Type 1"));                              // keypad and M1KAM
        Assert.True(Has(DataBusPlanner.Check(P(Direct(50, D("M1XIN", 5), D("M1XRFEG", 5)))), "error", "Type 2"));                            // two input expanders
        Assert.True(Has(DataBusPlanner.Check(P(Direct(50, D("Keypad", 2), D("Keypad", 2)))), "error", "Address 2 is used by 2"));
        // the rule holds across runs and hubs
        var across = DataBusPlanner.Check(P(Direct(50, D("M1XIN", 6)), Direct(50, D("M1XIN", 6))));
        Assert.True(Has(across, "error", "Address 6"));
    }

    [Theory]
    [InlineData("Keypad", 0, true)] [InlineData("Keypad", 16, false)] [InlineData("Keypad", 17, true)]
    [InlineData("M1XIN", 1, true)] [InlineData("M1XIN", 2, false)] [InlineData("M1XIN", 13, false)] [InlineData("M1XIN", 14, true)]
    [InlineData("M1XRFTWM", 6, true)] [InlineData("M1XRFTWM", 5, false)] [InlineData("M1XSP", 8, true)] [InlineData("M1XOVR", 14, true)]
    public void Address_Ranges(string model, int address, bool error)
    {
        var d = new List<BusDevice> { D(model, address) };
        if (model == "M1XRFTWM") d.Add(D("M1XRFTWM", 2));       // keep the primary rule out of the way
        Assert.Equal(error, DataBusPlanner.Check(P(Direct(50, d.ToArray()))).Issues.Any(i => i.Level == "error" && i.Text.Contains("addresses run from")));
    }

    [Fact]
    public void Primary_Transceiver_Must_Be_At_Address_2_And_At_Most_Four()
    {
        Assert.True(Has(DataBusPlanner.Check(P(Direct(50, D("M1XRFTWM", 3)))), "error", "primary M1XRFTWM transceiver must be at address 2"));
        Assert.Equal(0, DataBusPlanner.Check(P(Direct(50, D("M1XRFTWM", 2), D("M1XRFTWM", 3), D("M1XRFTWM", 4), D("M1XRFTWM", 5)))).Errors);
    }

    [Fact]
    public void Hardwired_Expanders_Cost_Wireless_Groups_And_Clash_With_Wireless_Zones()
    {
        var plan = P(Direct(50, D("M1XRFTWM", 2), D("M1XIN", 3, "A"), D("M1XIN", 5, "B")));
        var r = DataBusPlanner.Check(plan);
        Assert.True(Has(r, "info", "at most 112 wireless zones remain"));                                   // (9 - 2) x 16
        plan.WirelessGroups = new() { 5 };                                                                    // zones 65-80 are wireless in the account
        var clash = DataBusPlanner.Check(plan);
        Assert.True(Has(clash, "error", "zones 65-80")); Assert.Equal(1, clash.Errors);
        Assert.Equal((17, 32), DataBusPlanner.ZonesAt(2)); Assert.Equal((193, 208), DataBusPlanner.ZonesAt(13));
    }

    [Fact]
    public void Direct_Length_Limit_And_Missing_Devices()
    {
        Assert.True(Has(DataBusPlanner.Check(P(Direct(2500, D("Keypad", 1)), Direct(1600, D("Keypad", 2)))), "error", "4000 ft"));
        Assert.True(Has(DataBusPlanner.Check(P(Direct(3300, D("Keypad", 1)))), "warn", "close to"));
        Assert.True(Has(DataBusPlanner.Check(P(Direct(100))), "error", "no devices"));
    }

    [Fact]
    public void Hub_Home_Runs_Count_Double_And_Jacks_Are_Planned_In_Order()
    {
        var hub = Hub(0, R("Hall", 100, D("Keypad", 1)), R("Garage", 150, D("M1XIN", 3)), R("Up", 50, D("Keypad", 2)));
        var r = DataBusPlanner.Check(P(Dbh(2, hub)));
        Assert.Equal(2 + 2 * (100 + 150 + 50), r.MainBusFt); Assert.Equal(0, r.Errors); Assert.True(r.Jp3);
        var c = string.Join("\n", r.Checklist.SelectMany(g => g.Items));
        Assert.Contains("Hub 1, J1: home run \"Hall\"", c); Assert.Contains("Hub 1, J3: home run \"Up\"", c); Assert.Contains("Hub 1, J4: insert the RJ45 terminating plug", c);
        Assert.Contains("T568A", string.Join("\n", r.Checklist.Select(g => g.Title)));
        // a 2,000 ft home run is 4,000 ft of bus, and with the feed cable that is over the limit; 1,999 ft with a 1 ft feed is under it
        Assert.True(Has(DataBusPlanner.Check(P(Dbh(2, Hub(0, R("Far", 2000, D("Keypad", 1)))))), "error", "4000 ft"));
        Assert.Equal(0, DataBusPlanner.Check(P(Dbh(1, Hub(0, R("Far", 1999, D("Keypad", 1)))))).Errors);
    }

    [Fact]
    public void Last_Hub_Can_Use_All_Nine_Jacks_And_Ends_At_The_Device_On_The_Last_Jack()
    {
        var eight = Hub(0, Enumerable.Range(1, 8).Select(i => R("R" + i, 10, D("Keypad", i))).ToArray());
        var nine = Hub(0, Enumerable.Range(1, 9).Select(i => R("R" + i, 10, D("Keypad", i, "K" + i))).ToArray());
        var r8 = DataBusPlanner.Check(P(Dbh(1, eight)));
        Assert.Equal(0, r8.Errors); Assert.Contains("Hub 1, J9: insert the RJ45 terminating plug", string.Join("\n", r8.Checklist.SelectMany(g => g.Items)));
        // nine home runs fill every jack, so there is no plug: the device on J9 carries the terminating jumper (accepted by the installer)
        var r9 = DataBusPlanner.Check(P(Dbh(1, nine)));
        Assert.Equal(0, r9.Errors); Assert.True(Has(r9, "info", "Fit its terminating jumper instead of the plug"));
        var c = string.Join("\n", r9.Checklist.SelectMany(g => g.Items));
        Assert.Contains("no terminating plug. Fit the terminating jumper on the device on the last jack (J9): Keypad \"K9\" (address 9)", c); Assert.DoesNotContain("insert the RJ45 terminating plug", c);
        Assert.True(Has(DataBusPlanner.Check(P(Dbh(1, Hub(0, Enumerable.Range(1, 10).Select(i => R("R" + i, 10, D("Keypad", i))).ToArray())))), "error", "nine jacks"));
    }

    [Fact]
    public void Terminating_At_The_Last_Device_Can_Be_Chosen_Even_When_A_Jack_Is_Free()
    {
        var hub = Hub(0, R("A", 20, D("Keypad", 1, "First")), R("B", 20, D("M1XIN", 3, "Last")));
        hub.TerminateAtDevice = true;
        var r = DataBusPlanner.Check(P(Dbh(1, hub)));
        Assert.Equal(0, r.Errors); var c = string.Join("\n", r.Checklist.SelectMany(g => g.Items));
        Assert.Contains("J2): M1XIN \"Last\" (address 3)", c); Assert.DoesNotContain("insert the RJ45 terminating plug", c);
        var empty = Hub(0, R("A", 20)); empty.TerminateAtDevice = true;
        Assert.True(Has(DataBusPlanner.Check(P(Dbh(1, empty))), "error", "no device"));
        // a hub that is not the last one must keep J9 free for the crossover cable
        var full = Hub(5, Enumerable.Range(1, 9).Select(i => R("R" + i, 10, D("Keypad", i))).ToArray());
        Assert.True(Has(DataBusPlanner.Check(P(Dbh(1, full, Hub(0, R("B", 10, D("M1XIN", 3)))))), "error", "crossover cable to the next hub"));
    }

    [Fact]
    public void Two_Hubs_Use_A_Crossover_And_Only_The_Last_Is_Terminated()
    {
        var h1 = Hub(10, Enumerable.Range(1, 3).Select(i => R("A" + i, 20, D("Keypad", i))).ToArray());
        var h2 = Hub(0, R("B1", 20, D("M1XIN", 3)));
        var r = DataBusPlanner.Check(P(Dbh(1, h1, h2)));
        Assert.Equal(0, r.Errors); Assert.Equal(1 + 2 * (60 + 10 + 20), r.MainBusFt);
        var c = string.Join("\n", r.Checklist.SelectMany(g => g.Items));
        Assert.Contains("Hub 1, J4: crossover cable", c); Assert.Contains("J1 of hub 2", c); Assert.Contains("Hub 2, J2: insert the RJ45 terminating plug", c);
        Assert.DoesNotContain("Hub 1, J4: insert", c);
    }

    [Fact]
    public void Retrofit_Hub_Terminations_Per_Branch_And_Jp1_Jp3()
    {
        var one = DataBusPlanner.Check(P(Dbhr(2, new[] { R("Hall", 80, D("Keypad", 1)) }, new[] { R("A", 50, D("Keypad", 2)), R("B", 60, D("M1XIN", 3)) })));
        Assert.Equal(0, one.Errors); Assert.True(one.Jp3);
        var c = string.Join("\n", one.Checklist.SelectMany(g => g.Items));
        Assert.Contains("Branch 1: terminate the last device (Keypad (address 1)) AND fit JP2", c);
        Assert.Contains("Branch 2: terminate the last device of each home run and REMOVE JP3", c);
        Assert.Contains("Branch 3: not used", c); Assert.Contains("JP1 (terminates the input from the control): INSTALL", c);
        // two hubs: both JP1, JP3 off
        var two = DataBusPlanner.Check(P(Dbhr(1, new[] { R("A", 50, D("Keypad", 1)) }), Dbhr(1, new[] { R("B", 50, D("Keypad", 2)) })));
        Assert.False(two.Jp3); Assert.Equal(0, two.Errors);
    }

    [Fact]
    public void Retrofit_Hub_Limits()
    {
        var long1 = DataBusPlanner.Check(P(Dbhr(2, new[] { R("A", 2500, D("Keypad", 1)), R("B", 1600, D("Keypad", 2)) })));
        Assert.True(Has(long1, "error", "branch 1 is 4100 ft"));
        Assert.True(Has(DataBusPlanner.Check(P(Dbhr(15, new[] { R("A", 50, D("Keypad", 1)) }))), "error", "cannot be remoted"));
        Assert.True(Has(DataBusPlanner.Check(P(Dbhr(5, new[] { R("A", 50, D("Keypad", 1)) }))), "warn", "short 4-conductor"));
        var three = DataBusPlanner.Check(P(Dbhr(2, new[] { R("A", 5, D("Keypad", 1)), R("B", 5, D("Keypad", 2)), R("C", 5, D("Keypad", 3)) })));
        Assert.True(Has(three, "error", "at most 2"));
        Assert.True(Has(DataBusPlanner.Check(P(Dbhr(2, new[] { R("Empty", 5) }))), "error", "no device to terminate"));
        // branch cable does not add to the main bus: only the feed does
        Assert.Equal(2, DataBusPlanner.Check(P(Dbhr(2, new[] { R("A", 900, D("Keypad", 1)) }))).MainBusFt);
    }

    [Fact]
    public void Power_Is_Checked_Only_When_Currents_Are_Entered()
    {
        Assert.True(Has(DataBusPlanner.Check(P(Direct(50, D("Keypad", 1)))), "info", "No device currents"));
        Assert.True(Has(DataBusPlanner.Check(P(Direct(50, D("Keypad", 1, "", 700), D("Keypad", 2, "", 700)))), "error", "1.25 A"));
        Assert.True(Has(DataBusPlanner.Check(P(Direct(50, D("Keypad", 1, "", 1100)))), "warn", "near the 1.25 A"));
        // the retrofit hub adds 30 mA plus 20 mA for each branch in use
        var r = DataBusPlanner.Check(P(Dbhr(1, new[] { R("A", 5, D("Keypad", 1, "", 100)) }, new[] { R("B", 5, D("Keypad", 2, "", 100)) })));
        Assert.Equal(100 + 100 + 30 + 40, r.CurrentMa);
    }

    [Fact]
    public void Empty_Plan_Asks_For_A_Run()
    {
        var r = DataBusPlanner.Check(P());
        Assert.Equal(0, r.Errors); Assert.Contains(r.Issues, i => i.Level == "info");
    }

    [Fact]
    public void Switch_Settings_Appear_For_Expanders_And_Transceivers()
    {
        var r = DataBusPlanner.Check(P(Direct(50, D("M1XRFTWM", 2), D("M1XIN", 4, "Shed"))));
        var c = string.Join("\n", r.Checklist.Single(g => g.Title == "Addresses to set").Items);
        Assert.Contains("zones 49-64; S1 off, S2 off, S3 on, S4 off", c); Assert.Contains("primary transceiver; S1 off, S2 on, S3 off, S4 off", c); Assert.Contains("Bus Module Enrollment", c);
    }
}
