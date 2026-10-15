{ parity probe: a-28 }
unit P_a_28;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  a := nil;
  Show('a[0]', a[0]);
end;
end.
