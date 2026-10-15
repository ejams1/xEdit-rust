{ parity probe: r-10 }
unit P_r_10;

function Initialize: integer;
begin
  AddMessage('x ' + IntToStr(TFile.Count));
end;
end.
