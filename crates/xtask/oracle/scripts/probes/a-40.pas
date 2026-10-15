{ parity probe: a-40 }
unit P_a_40;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  Show('a[2147483648]', a[2147483648]);
end;
end.
