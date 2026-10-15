{ parity probe: r-05 }
unit P_r_05;

function Initialize: integer;
begin
  TFile.WriteAllText(wbTempPath + 'r-05.txt', 'x');
  AddMessage('wrote');
end;
end.
