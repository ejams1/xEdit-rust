{ parity probe: a-32 }
unit P_a_32;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  Inc(a[1]);
  Show('a[1]', a[1]);
  Inc(a[1], 5);
  Show('a[1]', a[1]);
end;
end.
