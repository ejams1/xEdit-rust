{ parity probe: x-091 }
unit P_x_091;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  AddMessage(BoolToStr(True, True));
  AddMessage('after');
end;
end.
