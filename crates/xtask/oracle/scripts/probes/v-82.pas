{ parity probe: v-82 }
unit P_v_82;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := Null <> 1;
  Show('Null <> 1', v);
end;
end.
