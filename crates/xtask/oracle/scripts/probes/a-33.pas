{ parity probe: a-33 }
unit P_a_33;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2, 0..2] of Integer;
begin
  Show('Length', Length(a));
end;
end.
