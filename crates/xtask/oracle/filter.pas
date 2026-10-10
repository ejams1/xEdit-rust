// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The oracle side of `cargo xtask parity filter`. The script mode of the GUI
// has the filter variables of the main form and the `ApplyFilter` function
// of the script host (the ones `Apply filter for cleaning.pas` uses), so the
// harness generates one block per filter of the scenario: the variables of
// every option of the filter dialog (`TfrmFilterOptions`) as the block sets
// them, then `ApplyFilter`, whose `FilterPreset` skips the dialog. The
// counts land in the message log of the main form, which the harness reads:
// `[file] Filtered n of m records` per partly filtered file and the closing
// `Done: Applying Filter, [Pass 1] Processed Records: ..., [Pass 2] ...,
// Remaining unfiltered nodes: ...`. A scenario that filters by the reachable
// status clicks `mniNavBuildReachable` first ("Build Reachable Info"), the
// menu item that sets `ReachableBuild`.
unit OracleFilter;

function Initialize: Integer;
var
  log: TStringList;
begin
  Result := 0;
  log := TStringList.Create;
  try
    try
      {{ACTIONS}}
      log.Add('done');
    except
      on E: Exception do
        log.Add('error: ' + E.Message);
    end;
  finally
    log.SaveToFile('{{WORK}}status.txt');
    log.Free;
  end;
  log := TStringList.Create;
  try
    log.Add('done');
    log.SaveToFile('{{WORK}}done.txt');
  finally
    log.Free;
  end;
end;

end.
