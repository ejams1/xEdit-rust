{ parity probe: x-260 }
unit P_x_260;

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
  v := IntToStr(-4294967296); AddMessage(v);
  AddMessage('after');
end;
end.
