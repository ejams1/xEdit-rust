{ parity probe: v-83 }
unit P_v_83;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := Null <> Null;
  Show('Null <> Null', v);
end;
end.
