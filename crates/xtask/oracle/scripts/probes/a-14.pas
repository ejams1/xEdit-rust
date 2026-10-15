{ parity probe: a-14 }
unit P_a_14;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array of Integer;
begin
  Show('a[0]', a[0]);
end;
end.
