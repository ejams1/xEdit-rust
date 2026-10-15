{ parity probe: r-09 }
unit P_r_09;

function Initialize: integer;
var
  s: TStringList;
begin
  AddMessage('type ' + IntToStr(VarType(TStringList)));
end;
end.
