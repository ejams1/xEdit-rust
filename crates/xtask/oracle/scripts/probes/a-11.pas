{ parity probe: a-11 }
unit P_a_11;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  Show('Length', Length(a));
  Show('High', High(a));
  Show('Low', Low(a));
  Show('a[0]', a[0]);
end;
end.
