{ parity probe: v-slot-05 }
unit P_v_slot_05;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  b: Boolean;
begin
  b := 1;
  v := b;
  Show('b', v);
end;
end.
