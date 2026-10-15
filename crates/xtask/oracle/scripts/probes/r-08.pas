{ parity probe: r-08 }
unit P_r_08;

function Initialize: integer;
var
  f: TFile;
begin
  AddMessage('type ' + IntToStr(VarType(f)));
end;
end.
