{ parity probe: e-id-199 }
unit P_e_id_199;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  x: Variant;
  y: Integer;
begin
  y := 1;
  x := y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y + y;
  AddMessage('x = ' + VarToStr(x));
end;
end.
