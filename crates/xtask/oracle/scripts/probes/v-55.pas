{ parity probe: v-55 }
unit P_v_55;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 4294967296 + 1;
  Show('4294967296 + 1', v);
end;
end.
