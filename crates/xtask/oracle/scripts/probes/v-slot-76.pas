{ parity probe: v-slot-76 }
unit P_v_slot_76;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  b: Boolean;
begin
  v := b;
  Show('b', v);
end;
end.
