{ parity probe: x-187 }
unit P_x_187;

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
  i := True; AddMessage(IntToStr(i));
  AddMessage('after');
end;
end.
